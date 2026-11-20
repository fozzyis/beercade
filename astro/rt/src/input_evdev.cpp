#include "astro_rt/input_events.h"
#include "astro_rt/spsc_ring.hpp"

#include <dirent.h>
#include <fcntl.h>
#include <linux/input.h>
#include <poll.h>
#include <pthread.h>
#include <sys/eventfd.h>
#include <sys/inotify.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

#include <algorithm>
#include <atomic>
#include <cerrno>
#include <climits>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <ctime>

using namespace astro_rt;

namespace {

constexpr std::uint32_t RING_CAPACITY = 8192;
SpscRing<astro_input_event, RING_CAPACITY> g_ring;

astro_pad_state g_pads[ASTRO_INPUT_MAX_PORTS]{};
astro_input_stats g_stats{};

std::atomic<bool> g_running{false};
std::atomic<bool> g_stop{false};
pthread_t g_thread_id{};
astro_input_config g_cfg{};

struct AxisCalibration {
    bool valid = false;
    int32_t minimum = 0;
    int32_t maximum = 0;
};

struct DeviceState {
    int fd = -1;
    uint16_t slot = 0;
    uint16_t assigned_port = 0xFFFF; // 0xFFFF means unassigned
    bool has_mono_clock = false;
    char dev_name[32]{};
    AxisCalibration axes[ASTRO_INPUT_MAX_AXES]{};
};

constexpr uint16_t kMappedAxes[6] = {
    ABS_X, ABS_Y, ABS_Z, ABS_RX, ABS_RY, ABS_RZ
};

DeviceState g_devices[ASTRO_INPUT_MAX_DEVICES]{};
int g_inotify_fd = -1;
int g_inotify_wd = -1;
int g_wake_fd = -1;

int g_shm_fd = -1;
astro_input_shm_stats* g_shm_stats = nullptr;
uint32_t g_hist_buckets[ASTRO_RT_HIST_BUCKETS]{};
uint32_t g_hist_max_us = 0;
uint32_t g_shm_seq = 0;
uint32_t g_event_seq = 0;

void log_msg(astro_input_log_level lvl, const char* msg) {
    if (lvl <= g_cfg.log_level) {
        if (lvl == ASTRO_LOG_ERROR) {
            std::fprintf(stderr, "[astro_rt] ERROR: %s\n", msg);
        } else if (lvl == ASTRO_LOG_INFO) {
            std::printf("[astro_rt] INFO: %s\n", msg);
        }
    }
}

void log_errno(astro_input_log_level lvl, const char* msg) {
    if (lvl <= g_cfg.log_level) {
        int err = errno;
        if (lvl == ASTRO_LOG_ERROR) {
            std::fprintf(stderr, "[astro_rt] ERROR: %s: %s (%d)\n", msg, std::strerror(err), err);
        } else if (lvl == ASTRO_LOG_INFO) {
            std::printf("[astro_rt] INFO: %s: %s (%d)\n", msg, std::strerror(err), err);
        }
    }
}

void close_fd_checked(int& fd) {
    if (fd >= 0) {
        if (close(fd) != 0 && errno != EBADF) {
            log_errno(ASTRO_LOG_ERROR, "close() failed");
        }
        fd = -1;
    }
}

int map_axis_code(uint16_t code) {
    switch (code) {
        case ABS_X:  return 0;
        case ABS_Y:  return 1;
        case ABS_Z:  return 2;
        case ABS_RX: return 3;
        case ABS_RY: return 4;
        case ABS_RZ: return 5;
        default:     return -1;
    }
}

int map_btn_code(uint16_t code) {
    switch (code) {
        case BTN_EAST:       return ASTRO_BTN_B;
        case BTN_SOUTH:      return ASTRO_BTN_A;
        case BTN_WEST:       return ASTRO_BTN_Y;
        case BTN_NORTH:      return ASTRO_BTN_X;
        case BTN_START:      return ASTRO_BTN_START;
        case BTN_SELECT:     return ASTRO_BTN_SELECT;
        case BTN_DPAD_UP:    return ASTRO_BTN_UP;
        case BTN_DPAD_DOWN:  return ASTRO_BTN_DOWN;
        case BTN_DPAD_LEFT:  return ASTRO_BTN_LEFT;
        case BTN_DPAD_RIGHT: return ASTRO_BTN_RIGHT;
        case BTN_TL:         return ASTRO_BTN_L;
        case BTN_TR:         return ASTRO_BTN_R;
        case BTN_TL2:        return ASTRO_BTN_L2;
        case BTN_TR2:        return ASTRO_BTN_R2;
        case BTN_THUMBL:     return ASTRO_BTN_L3;
        case BTN_THUMBR:     return ASTRO_BTN_R3;
        default:             return -1;
    }
}

bool is_event_node_name(const char* name) {
    if (!name || std::strncmp(name, "event", 5) != 0) {
        return false;
    }
    if (name[5] == '\0') {
        return false;
    }
    for (const char* p = name + 5; *p != '\0'; ++p) {
        if (*p < '0' || *p > '9') {
            return false;
        }
    }
    return true;
}

unsigned bucket_for_us(uint64_t dt_us) {
    if (dt_us <= 1ULL) {
        return 0u;
    }
    if (dt_us >= (1ULL << 31)) {
        return ASTRO_RT_HIST_BUCKETS - 1u;
    }
    return 31u - static_cast<unsigned>(__builtin_clz(static_cast<uint32_t>(dt_us)));
}

void record_latency_sample(uint64_t dt_us) {
    const unsigned b = bucket_for_us(dt_us);
    ++g_hist_buckets[b];
    const uint32_t clamped = static_cast<uint32_t>(std::min<uint64_t>(dt_us, UINT32_MAX));
    if (clamped > g_hist_max_us) {
        g_hist_max_us = clamped;
    }
    __atomic_store_n(&g_stats.last_wake_latency_us, clamped, __ATOMIC_RELAXED);
}

void publish_stats_shm() {
    if (!g_shm_stats) {
        return;
    }

    uint32_t p50 = 0;
    uint32_t p95 = 0;
    uint32_t p99 = 0;
    astro_input_compute_percentiles(g_hist_buckets, ASTRO_RT_HIST_BUCKETS, &p50, &p95, &p99);

    ++g_shm_seq;
    if ((g_shm_seq & 1u) == 0u) {
        ++g_shm_seq;
    }
    __atomic_store_n(&g_shm_stats->seq, g_shm_seq, __ATOMIC_RELEASE);
    __atomic_thread_fence(__ATOMIC_RELEASE);

    g_shm_stats->magic = ASTRO_RT_STATS_MAGIC;
    g_shm_stats->version = ASTRO_RT_STATS_VERSION;
    g_shm_stats->poll_rate_hz = 1000u;
    g_shm_stats->p50_us = p50;
    g_shm_stats->p95_us = p95;
    g_shm_stats->p99_us = p99;
    g_shm_stats->max_us = g_hist_max_us;
    g_shm_stats->events_total = __atomic_load_n(&g_stats.events_total, __ATOMIC_RELAXED);
    g_shm_stats->events_dropped = g_ring.producer_dropped();
    g_shm_stats->devices_active = __atomic_load_n(&g_stats.devices_active, __ATOMIC_RELAXED);
    g_shm_stats->hotplug_adds = __atomic_load_n(&g_stats.hotplug_adds, __ATOMIC_RELAXED);
    g_shm_stats->hotplug_removes = __atomic_load_n(&g_stats.hotplug_removes, __ATOMIC_RELAXED);
    for (unsigned i = 0; i < ASTRO_RT_HIST_BUCKETS; ++i) {
        g_shm_stats->buckets[i] = g_hist_buckets[i];
    }

    ++g_shm_seq;
    __atomic_store_n(&g_shm_stats->seq, g_shm_seq, __ATOMIC_RELEASE);
}

void open_stats_shm() {
    int fd = shm_open(ASTRO_RT_STATS_SHM_NAME, O_CREAT | O_RDWR | O_CLOEXEC, 0644);
    if (fd < 0) {
        log_errno(ASTRO_LOG_ERROR, "shm_open(/astro_rt_stats) failed");
        return;
    }
    if (ftruncate(fd, static_cast<off_t>(sizeof(astro_input_shm_stats))) != 0) {
        log_errno(ASTRO_LOG_ERROR, "ftruncate(/astro_rt_stats) failed");
        close_fd_checked(fd);
        return;
    }
    void* ptr = mmap(nullptr, sizeof(astro_input_shm_stats), PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (ptr == MAP_FAILED) {
        log_errno(ASTRO_LOG_ERROR, "mmap(/astro_rt_stats) failed");
        close_fd_checked(fd);
        return;
    }
    g_shm_fd = fd;
    g_shm_stats = static_cast<astro_input_shm_stats*>(ptr);
    std::memset(g_shm_stats, 0, sizeof(astro_input_shm_stats));
    g_shm_seq = 0;
    publish_stats_shm();
}

void close_stats_shm() {
    if (g_shm_stats) {
        publish_stats_shm();
        if (munmap(g_shm_stats, sizeof(astro_input_shm_stats)) != 0) {
            log_errno(ASTRO_LOG_ERROR, "munmap(/astro_rt_stats) failed");
        }
        g_shm_stats = nullptr;
    }
    close_fd_checked(g_shm_fd);
}

bool port_in_use(uint16_t port) {
    for (unsigned s = 0; s < ASTRO_INPUT_MAX_DEVICES; ++s) {
        if (g_devices[s].fd >= 0 && g_devices[s].assigned_port == port) {
            return true;
        }
    }
    return false;
}

void assign_port_if_needed(DeviceState& dev, uint16_t type, int32_t value) {
    if (type != EV_KEY || value != 1 || dev.assigned_port != 0xFFFF) {
        return;
    }
    for (uint16_t port = 0; port < ASTRO_INPUT_MAX_PORTS; ++port) {
        if (!port_in_use(port)) {
            dev.assigned_port = port;
            __atomic_store_n(&g_pads[port].device_slot, dev.slot, __ATOMIC_RELAXED);
            __atomic_store_n(&g_pads[port].connected, static_cast<uint16_t>(1u), __ATOMIC_RELEASE);
            char buf[128];
            std::snprintf(buf, sizeof(buf), "Assigned evdev slot %u to player port %u",
                          static_cast<unsigned>(dev.slot), static_cast<unsigned>(port));
            log_msg(ASTRO_LOG_INFO, buf);
            return;
        }
    }
}

void query_abs_calibration(int fd, DeviceState& dev) {
    for (size_t i = 0; i < ASTRO_INPUT_MAX_AXES; ++i) {
        dev.axes[i] = AxisCalibration{};
    }
    for (size_t i = 0; i < sizeof(kMappedAxes) / sizeof(kMappedAxes[0]); ++i) {
        struct input_absinfo absinfo{};
        if (ioctl(fd, EVIOCGABS(kMappedAxes[i]), &absinfo) == 0 &&
            absinfo.maximum > absinfo.minimum) {
            dev.axes[i].valid = true;
            dev.axes[i].minimum = absinfo.minimum;
            dev.axes[i].maximum = absinfo.maximum;
        }
    }
}

bool open_evdev_device(const char* name, bool is_hotplug) {
    if (!is_event_node_name(name)) {
        return false;
    }
    const size_t name_len = std::strlen(name);
    if (name_len >= sizeof(DeviceState::dev_name)) {
        return false;
    }

    for (unsigned s = 0; s < ASTRO_INPUT_MAX_DEVICES; ++s) {
        if (g_devices[s].fd >= 0 && std::strcmp(g_devices[s].dev_name, name) == 0) {
            return false; // already open (e.g. IN_ATTRIB after IN_CREATE/coldplug)
        }
    }

    int free_slot = -1;
    for (unsigned s = 0; s < ASTRO_INPUT_MAX_DEVICES; ++s) {
        if (g_devices[s].fd < 0) {
            free_slot = static_cast<int>(s);
            break;
        }
    }
    if (free_slot < 0) {
        return false;
    }

    char path[sizeof("/dev/input/") + sizeof(DeviceState::dev_name)];
    std::memcpy(path, "/dev/input/", sizeof("/dev/input/") - 1);
    std::memcpy(path + sizeof("/dev/input/") - 1, name, name_len + 1);

    int fd = open(path, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) {
        return false;
    }
    if (ioctl(fd, EVIOCGRAB, 1) != 0) {
        close_fd_checked(fd);
        return false;
    }

    unsigned int clockid = CLOCK_MONOTONIC;
    const bool has_mono = (ioctl(fd, EVIOCSCLOCKID, &clockid) == 0);
    if (!has_mono) {
        log_errno(ASTRO_LOG_ERROR, "EVIOCSCLOCKID(CLOCK_MONOTONIC) failed");
    }

    DeviceState& dev = g_devices[free_slot];
    dev.fd = fd;
    dev.slot = static_cast<uint16_t>(free_slot);
    dev.assigned_port = 0xFFFF;
    dev.has_mono_clock = has_mono;
    std::memcpy(dev.dev_name, name, name_len + 1);
    query_abs_calibration(fd, dev);

    __atomic_fetch_add(&g_stats.devices_active, 1u, __ATOMIC_RELAXED);
    if (is_hotplug) {
        __atomic_fetch_add(&g_stats.hotplug_adds, 1u, __ATOMIC_RELAXED);
        char buf[128];
        std::snprintf(buf, sizeof(buf), "Hotplug added %s at slot %u",
                      dev.dev_name, static_cast<unsigned>(dev.slot));
        log_msg(ASTRO_LOG_INFO, buf);
    }
    return true;
}

void remove_device_slot(unsigned slot, bool count_hotplug_remove) {
    if (slot >= ASTRO_INPUT_MAX_DEVICES) {
        return;
    }
    DeviceState& dev = g_devices[slot];
    if (dev.fd < 0) {
        return;
    }

    const uint16_t port = dev.assigned_port;
    if (ioctl(dev.fd, EVIOCGRAB, 0) != 0 && errno != ENODEV && errno != EBADF && errno != EINVAL) {
        log_errno(ASTRO_LOG_ERROR, "EVIOCGRAB release failed");
    }
    close_fd_checked(dev.fd);
    dev.assigned_port = 0xFFFF;
    dev.has_mono_clock = false;
    dev.dev_name[0] = '\0';
    for (size_t i = 0; i < ASTRO_INPUT_MAX_AXES; ++i) {
        dev.axes[i] = AxisCalibration{};
    }

    const uint32_t prev_active = __atomic_load_n(&g_stats.devices_active, __ATOMIC_RELAXED);
    if (prev_active > 0) {
        __atomic_fetch_sub(&g_stats.devices_active, 1u, __ATOMIC_RELAXED);
    }
    if (count_hotplug_remove) {
        __atomic_fetch_add(&g_stats.hotplug_removes, 1u, __ATOMIC_RELAXED);
    }

    if (port < ASTRO_INPUT_MAX_PORTS && !port_in_use(port)) {
        __atomic_store_n(&g_pads[port].connected, static_cast<uint16_t>(0u), __ATOMIC_RELEASE);
    }
}

void remove_device_by_name(const char* name) {
    if (!name || name[0] == '\0') {
        return;
    }
    for (unsigned s = 0; s < ASTRO_INPUT_MAX_DEVICES; ++s) {
        if (g_devices[s].fd >= 0 && std::strcmp(g_devices[s].dev_name, name) == 0) {
            char buf[128];
            std::snprintf(buf, sizeof(buf), "Hotplug removed %s from slot %u", name, s);
            log_msg(ASTRO_LOG_INFO, buf);
            remove_device_slot(s, true);
            return;
        }
    }
}

bool handle_inotify_events() {
    if (g_inotify_fd < 0) {
        return false;
    }
    bool changed = false;
    alignas(struct inotify_event) char buf[4096];
    for (;;) {
        ssize_t n = read(g_inotify_fd, buf, sizeof(buf));
        if (n < 0) {
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                break;
            }
            if (errno == EINTR) {
                continue;
            }
            log_errno(ASTRO_LOG_ERROR, "inotify read() failed");
            break;
        }
        if (n == 0) {
            break;
        }
        ssize_t offset = 0;
        while (offset + static_cast<ssize_t>(sizeof(struct inotify_event)) <= n) {
            const auto* ev = reinterpret_cast<const struct inotify_event*>(buf + offset);
            const ssize_t step = static_cast<ssize_t>(sizeof(struct inotify_event)) +
                                 static_cast<ssize_t>(ev->len);
            if (step <= 0 || offset + step > n) {
                break;
            }
            if (ev->len > 0 && is_event_node_name(ev->name)) {
                if (ev->mask & IN_DELETE) {
                    remove_device_by_name(ev->name);
                    changed = true;
                } else if (ev->mask & (IN_CREATE | IN_ATTRIB)) {
                    if (open_evdev_device(ev->name, true)) {
                        changed = true;
                    }
                }
            }
            offset += step;
        }
    }
    return changed;
}

void* evdev_thread_main(void* arg) {
    (void)arg;
    // pfds[0] = inotify (/dev/input)
    // pfds[1..ASTRO_INPUT_MAX_DEVICES] = fixed evdev slots
    // pfds[1 + ASTRO_INPUT_MAX_DEVICES] = stop wakeup eventfd
    constexpr size_t kWakeIndex = 1u + ASTRO_INPUT_MAX_DEVICES;
    struct pollfd pfds[2u + ASTRO_INPUT_MAX_DEVICES]{};

    while (!g_stop.load(std::memory_order_acquire)) {
        pfds[0].fd = g_inotify_fd;
        pfds[0].events = (g_inotify_fd >= 0) ? POLLIN : 0;
        pfds[0].revents = 0;

        for (unsigned s = 0; s < ASTRO_INPUT_MAX_DEVICES; ++s) {
            pfds[1u + s].fd = g_devices[s].fd;
            pfds[1u + s].events = (g_devices[s].fd >= 0) ? POLLIN : 0;
            pfds[1u + s].revents = 0;
        }

        pfds[kWakeIndex].fd = g_wake_fd;
        pfds[kWakeIndex].events = (g_wake_fd >= 0) ? POLLIN : 0;
        pfds[kWakeIndex].revents = 0;

        const int timeout = (g_cfg.ppoll_timeout_ms == 0)
                                ? -1
                                : static_cast<int>(std::min<uint32_t>(g_cfg.ppoll_timeout_ms,
                                                                      static_cast<uint32_t>(INT_MAX)));
        const int ret = poll(pfds, 2u + ASTRO_INPUT_MAX_DEVICES, timeout);
        if (ret < 0) {
            if (errno == EINTR) {
                continue;
            }
            log_errno(ASTRO_LOG_ERROR, "poll() failed");
            break;
        }
        if (ret == 0) {
            continue;
        }

        if (pfds[kWakeIndex].revents & POLLIN) {
            uint64_t val = 0;
            const ssize_t rd = read(g_wake_fd, &val, sizeof(val));
            if (rd < 0 && errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR) {
                log_errno(ASTRO_LOG_ERROR, "wake eventfd read() failed");
            }
        }
        if (g_stop.load(std::memory_order_acquire)) {
            break;
        }

        struct timespec ts_wake{};
        uint64_t wake_us = 0;
        bool have_wake_ts = false;
        if (clock_gettime(CLOCK_MONOTONIC, &ts_wake) == 0) {
            wake_us = static_cast<uint64_t>(ts_wake.tv_sec) * 1000000ULL +
                      static_cast<uint64_t>(ts_wake.tv_nsec) / 1000ULL;
            have_wake_ts = true;
        } else {
            log_errno(ASTRO_LOG_ERROR, "clock_gettime(CLOCK_MONOTONIC) failed");
        }

        bool shm_dirty = false;

        if (pfds[0].revents & POLLIN) {
            if (handle_inotify_events()) {
                shm_dirty = true;
            }
        } else if (pfds[0].revents & (POLLERR | POLLHUP | POLLNVAL)) {
            log_msg(ASTRO_LOG_ERROR, "inotify fd error; disabling runtime hotplug watch");
            close_fd_checked(g_inotify_fd);
            g_inotify_wd = -1;
        }

        for (unsigned s = 0; s < ASTRO_INPUT_MAX_DEVICES; ++s) {
            const short rev = pfds[1u + s].revents;
            if (rev == 0 || g_devices[s].fd < 0) {
                continue;
            }

            if (rev & POLLIN) {
                struct input_event evs[64];
                for (;;) {
                    const ssize_t bytes = read(g_devices[s].fd, evs, sizeof(evs));
                    if (bytes > 0) {
                        const size_t count = static_cast<size_t>(bytes) / sizeof(struct input_event);
                        for (size_t j = 0; j < count; ++j) {
                            if (evs[j].type == EV_SYN || evs[j].type == EV_MSC) {
                                continue;
                            }

                            astro_input_event aie{};
                            aie.ts_mono_us = static_cast<uint64_t>(evs[j].time.tv_sec) * 1000000ULL +
                                             static_cast<uint64_t>(evs[j].time.tv_usec);
                            aie.seq = ++g_event_seq;
                            aie.device_slot = g_devices[s].slot;
                            aie.type = evs[j].type;
                            aie.code = evs[j].code;
                            aie.value = evs[j].value;

                            if (aie.type == EV_ABS) {
                                const int axis = map_axis_code(aie.code);
                                if (axis >= 0 && axis < ASTRO_INPUT_MAX_AXES &&
                                    g_devices[s].axes[axis].valid) {
                                    aie.value = static_cast<int32_t>(astro_input_scale_abs(
                                        evs[j].value,
                                        g_devices[s].axes[axis].minimum,
                                        g_devices[s].axes[axis].maximum));
                                }
                            }

                            if (g_devices[s].has_mono_clock && have_wake_ts) {
                                const uint64_t delta_us = (wake_us >= aie.ts_mono_us)
                                                              ? (wake_us - aie.ts_mono_us)
                                                              : 0ULL;
                                record_latency_sample(delta_us);
                                shm_dirty = true;
                            }

                            assign_port_if_needed(g_devices[s], aie.type, aie.value);
                            aie.port = g_devices[s].assigned_port;

                            if (aie.port != 0xFFFF) {
                                if (g_ring.try_push(aie)) {
                                    __atomic_fetch_add(&g_stats.events_total, 1u, __ATOMIC_RELAXED);
                                }
                                shm_dirty = true;
                            }
                        }
                    } else if (bytes == 0) {
                        remove_device_slot(s, true);
                        shm_dirty = true;
                        break;
                    } else {
                        if (errno == EAGAIN || errno == EWOULDBLOCK) {
                            break;
                        }
                        if (errno == EINTR) {
                            continue;
                        }
                        if (errno == ENODEV || errno == EIO) {
                            __atomic_fetch_add(&g_stats.disconnect_errors, 1u, __ATOMIC_RELAXED);
                        } else {
                            log_errno(ASTRO_LOG_ERROR, "evdev read() failed");
                        }
                        remove_device_slot(s, true);
                        shm_dirty = true;
                        break;
                    }
                }
            }

            if (g_devices[s].fd >= 0 && (rev & (POLLERR | POLLHUP | POLLNVAL))) {
                if (rev & POLLERR) {
                    __atomic_fetch_add(&g_stats.disconnect_errors, 1u, __ATOMIC_RELAXED);
                }
                remove_device_slot(s, true);
                shm_dirty = true;
            }
        }

        if (shm_dirty) {
            publish_stats_shm();
        }
    }
    return nullptr;
}

} // namespace

extern "C" {

int16_t astro_input_scale_abs(int32_t value, int32_t minimum, int32_t maximum) {
    if (maximum <= minimum) {
        return 0;
    }
    int64_t v = value;
    if (v < minimum) {
        v = minimum;
    } else if (v > maximum) {
        v = maximum;
    }
    const int64_t span = static_cast<int64_t>(maximum) - static_cast<int64_t>(minimum);
    const int64_t pos = v - static_cast<int64_t>(minimum);
    int64_t scaled = (pos * 65535LL + span / 2LL) / span - 32768LL;
    if (scaled < INT16_MIN) {
        scaled = INT16_MIN;
    } else if (scaled > INT16_MAX) {
        scaled = INT16_MAX;
    }
    return static_cast<int16_t>(scaled);
}

void astro_input_compute_percentiles(const uint32_t* buckets,
                                     unsigned num_buckets,
                                     uint32_t* p50_us,
                                     uint32_t* p95_us,
                                     uint32_t* p99_us) {
    if (p50_us) *p50_us = 0;
    if (p95_us) *p95_us = 0;
    if (p99_us) *p99_us = 0;
    if (!buckets || num_buckets == 0) {
        return;
    }

    uint64_t total = 0;
    for (unsigned b = 0; b < num_buckets; ++b) {
        total += buckets[b];
    }
    if (total == 0) {
        return;
    }

    auto calc = [&](uint64_t pct) -> uint32_t {
        uint64_t target = (total * pct + 99ULL) / 100ULL;
        if (target == 0) {
            target = 1;
        }
        uint64_t cum = 0;
        for (unsigned b = 0; b < num_buckets; ++b) {
            const uint32_t cnt = buckets[b];
            if (cnt == 0) {
                continue;
            }
            const uint64_t prev_cum = cum;
            cum += cnt;
            if (cum >= target) {
                const uint64_t lo = (b == 0)
                                        ? 0ULL
                                        : (b >= 32 ? static_cast<uint64_t>(UINT32_MAX) : (1ULL << b));
                const uint64_t hi = (b == 0)
                                        ? 1ULL
                                        : (b >= 31 ? static_cast<uint64_t>(UINT32_MAX)
                                                   : ((1ULL << (b + 1)) - 1ULL));
                const uint64_t in_bucket = target - prev_cum;
                const uint64_t span = hi - lo;
                const uint64_t den = 2ULL * static_cast<uint64_t>(cnt);
#ifdef __SIZEOF_INT128__
                const unsigned __int128 num =
                    static_cast<unsigned __int128>(2ULL * in_bucket - 1ULL) *
                    static_cast<unsigned __int128>(span);
                const uint64_t offset = static_cast<uint64_t>((num + den / 2ULL) / den);
#else
                const uint64_t offset = static_cast<uint64_t>(
                    ((static_cast<double>(2ULL * in_bucket - 1ULL) * static_cast<double>(span)) +
                     static_cast<double>(den) * 0.5) /
                    static_cast<double>(den));
#endif
                const uint64_t val = lo + offset;
                return static_cast<uint32_t>(std::min<uint64_t>(val, UINT32_MAX));
            }
        }
        return 0;
    };

    if (p50_us) *p50_us = calc(50);
    if (p95_us) *p95_us = calc(95);
    if (p99_us) *p99_us = calc(99);
}

int astro_input_start(const struct astro_input_config* cfg) {
    if (!cfg) {
        return -EINVAL;
    }
    if (g_running.load(std::memory_order_acquire)) {
        return 0;
    }

    g_cfg = *cfg;

    if (g_cfg.mlockall_enable) {
        if (mlockall(MCL_CURRENT | MCL_FUTURE) != 0) {
            log_errno(ASTRO_LOG_ERROR, "Failed to mlockall()");
        }
    }

    for (unsigned s = 0; s < ASTRO_INPUT_MAX_DEVICES; ++s) {
        g_devices[s] = DeviceState{};
        g_devices[s].slot = static_cast<uint16_t>(s);
    }
    std::memset(g_pads, 0, sizeof(g_pads));
    std::memset(&g_stats, 0, sizeof(g_stats));
    std::memset(g_hist_buckets, 0, sizeof(g_hist_buckets));
    g_hist_max_us = 0;
    g_event_seq = 0;

    g_wake_fd = eventfd(0, EFD_NONBLOCK | EFD_CLOEXEC);
    if (g_wake_fd < 0) {
        const int err = errno;
        log_errno(ASTRO_LOG_ERROR, "eventfd() failed");
        return -err;
    }

    g_inotify_fd = inotify_init1(IN_NONBLOCK | IN_CLOEXEC);
    if (g_inotify_fd < 0) {
        log_errno(ASTRO_LOG_ERROR, "inotify_init1() failed");
    } else {
        g_inotify_wd = inotify_add_watch(g_inotify_fd, "/dev/input",
                                         IN_CREATE | IN_DELETE | IN_ATTRIB);
        if (g_inotify_wd < 0) {
            log_errno(ASTRO_LOG_ERROR, "inotify_add_watch(/dev/input) failed");
            close_fd_checked(g_inotify_fd);
        }
    }

    DIR* dir = opendir("/dev/input");
    if (dir) {
        while (struct dirent* ent = readdir(dir)) {
            if (is_event_node_name(ent->d_name)) {
                (void)open_evdev_device(ent->d_name, false);
            }
        }
        if (closedir(dir) != 0) {
            log_errno(ASTRO_LOG_ERROR, "closedir(/dev/input) failed");
        }
    } else {
        log_errno(ASTRO_LOG_ERROR, "opendir(/dev/input) failed");
    }

    open_stats_shm();
    g_stop.store(false, std::memory_order_release);

    pthread_attr_t attr;
    const int attr_rc = pthread_attr_init(&attr);
    int create_rc = -1;
    if (attr_rc == 0) {
        struct sched_param param{};
        param.sched_priority = g_cfg.fifo_priority;
        if (pthread_attr_setschedpolicy(&attr, SCHED_FIFO) != 0 ||
            pthread_attr_setschedparam(&attr, &param) != 0 ||
            pthread_attr_setinheritsched(&attr, PTHREAD_EXPLICIT_SCHED) != 0) {
            log_msg(ASTRO_LOG_ERROR, "Failed to configure SCHED_FIFO thread attributes");
        }

        cpu_set_t cpuset;
        CPU_ZERO(&cpuset);
        for (unsigned c = g_cfg.pin_cpu_first;
             c <= g_cfg.pin_cpu_last && c < static_cast<unsigned>(CPU_SETSIZE);
             ++c) {
            CPU_SET(c, &cpuset);
        }
        if (CPU_COUNT(&cpuset) > 0) {
            if (pthread_attr_setaffinity_np(&attr, sizeof(cpu_set_t), &cpuset) != 0) {
                log_msg(ASTRO_LOG_ERROR, "Failed to set RT thread CPU affinity");
            }
        }

        create_rc = pthread_create(&g_thread_id, &attr, evdev_thread_main, nullptr);
        if (pthread_attr_destroy(&attr) != 0) {
            log_errno(ASTRO_LOG_ERROR, "pthread_attr_destroy() failed");
        }
    }

    if (create_rc != 0) {
        if (attr_rc == 0) {
            log_msg(ASTRO_LOG_ERROR, "Failed to create RT thread with SCHED_FIFO, falling back");
        }
        create_rc = pthread_create(&g_thread_id, nullptr, evdev_thread_main, nullptr);
    }

    if (create_rc != 0) {
        for (unsigned s = 0; s < ASTRO_INPUT_MAX_DEVICES; ++s) {
            remove_device_slot(s, false);
        }
        close_stats_shm();
        close_fd_checked(g_inotify_fd);
        g_inotify_wd = -1;
        close_fd_checked(g_wake_fd);
        return -create_rc;
    }

    g_running.store(true, std::memory_order_release);
    return 0;
}

void astro_input_stop(void) {
    if (!g_running.exchange(false, std::memory_order_acq_rel)) {
        return;
    }

    g_stop.store(true, std::memory_order_release);
    if (g_wake_fd >= 0) {
        const uint64_t one = 1;
        const ssize_t wr = write(g_wake_fd, &one, sizeof(one));
        if (wr < 0 && errno != EAGAIN && errno != EWOULDBLOCK) {
            log_errno(ASTRO_LOG_ERROR, "wake eventfd write() failed");
        }
    }

    const int jrc = pthread_join(g_thread_id, nullptr);
    if (jrc != 0) {
        errno = jrc;
        log_errno(ASTRO_LOG_ERROR, "pthread_join() failed");
    }

    for (unsigned s = 0; s < ASTRO_INPUT_MAX_DEVICES; ++s) {
        remove_device_slot(s, false);
    }

    if (g_inotify_fd >= 0 && g_inotify_wd >= 0) {
        if (inotify_rm_watch(g_inotify_fd, g_inotify_wd) != 0 && errno != EINVAL && errno != EBADF) {
            log_errno(ASTRO_LOG_ERROR, "inotify_rm_watch() failed");
        }
    }
    g_inotify_wd = -1;
    close_fd_checked(g_inotify_fd);
    close_fd_checked(g_wake_fd);
    close_stats_shm();
}

unsigned astro_input_drain(struct astro_input_event* out, unsigned max_out) {
    const unsigned n = g_ring.drain(out, max_out);
    for (unsigned i = 0; i < n; ++i) {
        const uint16_t port = out[i].port;
        if (port < ASTRO_INPUT_MAX_PORTS) {
            if (out[i].type == EV_KEY) {
                const int bit = map_btn_code(out[i].code);
                if (bit >= 0 && bit < 32) {
                    if (out[i].value) {
                        g_pads[port].buttons |= (1U << bit);
                    } else {
                        g_pads[port].buttons &= ~(1U << bit);
                    }
                }
            } else if (out[i].type == EV_ABS) {
                const int axis = map_axis_code(out[i].code);
                if (axis >= 0 && axis < ASTRO_INPUT_MAX_AXES) {
                    int32_t v = out[i].value;
                    if (v < INT16_MIN) {
                        v = INT16_MIN;
                    } else if (v > INT16_MAX) {
                        v = INT16_MAX;
                    }
                    g_pads[port].axes[axis] = static_cast<int16_t>(v);
                }
            }
        }
    }
    return n;
}

unsigned astro_input_poll_frame(struct astro_pad_state* pads, unsigned max_ports) {
    struct astro_input_event evs[ASTRO_INPUT_DRAIN_MAX];
    (void)astro_input_drain(evs, ASTRO_INPUT_DRAIN_MAX);
    const unsigned n = std::min(max_ports, static_cast<unsigned>(ASTRO_INPUT_MAX_PORTS));
    if (pads) {
        for (unsigned i = 0; i < n; ++i) {
            pads[i] = g_pads[i];
            pads[i].connected = __atomic_load_n(&g_pads[i].connected, __ATOMIC_ACQUIRE);
        }
    }
    return n;
}

int astro_input_button(unsigned port, unsigned button_bit) {
    if (port >= ASTRO_INPUT_MAX_PORTS || button_bit >= 32) {
        return 0;
    }
    return (g_pads[port].buttons & (1U << button_bit)) ? 1 : 0;
}

int16_t astro_input_axis(unsigned port, unsigned axis) {
    if (port >= ASTRO_INPUT_MAX_PORTS || axis >= ASTRO_INPUT_MAX_AXES) {
        return 0;
    }
    return g_pads[port].axes[axis];
}

void astro_input_get_stats(struct astro_input_stats* out) {
    if (!out) {
        return;
    }
    out->events_total = __atomic_load_n(&g_stats.events_total, __ATOMIC_RELAXED);
    out->events_dropped = g_ring.producer_dropped();
    out->hotplug_adds = __atomic_load_n(&g_stats.hotplug_adds, __ATOMIC_RELAXED);
    out->hotplug_removes = __atomic_load_n(&g_stats.hotplug_removes, __ATOMIC_RELAXED);
    out->disconnect_errors = __atomic_load_n(&g_stats.disconnect_errors, __ATOMIC_RELAXED);
    out->devices_active = __atomic_load_n(&g_stats.devices_active, __ATOMIC_RELAXED);
    out->last_wake_latency_us = __atomic_load_n(&g_stats.last_wake_latency_us, __ATOMIC_RELAXED);
}

const char* astro_input_ident(void) {
    return "astro_rt_v1";
}

} // extern "C"
