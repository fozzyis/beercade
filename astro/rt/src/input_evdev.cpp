#include "astro_rt/input_events.h"
#include "astro_rt/spsc_ring.hpp"
#include <pthread.h>
#include <fcntl.h>
#include <unistd.h>
#include <poll.h>
#include <linux/input.h>
#include <sys/mman.h>
#include <sys/ioctl.h>
#include <cerrno>
#include <cstring>
#include <cstdio>
#include <atomic>
#include <vector>
#include <algorithm>
#include <dirent.h>

using namespace astro_rt;

namespace {
    constexpr std::uint32_t RING_CAPACITY = 8192;
    SpscRing<astro_input_event, RING_CAPACITY> g_ring;
    
    astro_pad_state g_pads[ASTRO_INPUT_MAX_PORTS]{};
    astro_input_stats g_stats{};
    
    std::atomic<bool> g_stop{false};
    pthread_t g_thread_id{};
    astro_input_config g_cfg{};
    
    struct DeviceState {
        int fd = -1;
        uint16_t slot = 0;
        uint16_t assigned_port = 0xFFFF; // 0xFFFF means unassigned
    };
    
    std::vector<DeviceState> g_devices;
    
    // Port assignment logic ("first to press start")
    std::atomic<uint16_t> g_next_available_port{0};
    
    void log_msg(astro_input_log_level lvl, const char* msg) {
        if (lvl <= g_cfg.log_level) {
            if (lvl == ASTRO_LOG_ERROR) fprintf(stderr, "[astro_rt] ERROR: %s\n", msg);
            else if (lvl == ASTRO_LOG_INFO) printf("[astro_rt] INFO: %s\n", msg);
        }
    }
    
    void assign_port_if_needed(uint16_t slot, uint16_t type, int32_t value) {
        // Assign port on the first key press (value == 1).
        if (type == EV_KEY && value == 1) { 
            for (auto& dev : g_devices) {
                if (dev.slot == slot && dev.assigned_port == 0xFFFF) {
                    uint16_t port = g_next_available_port.fetch_add(1, std::memory_order_relaxed);
                    if (port < ASTRO_INPUT_MAX_PORTS) {
                        dev.assigned_port = port;
                        g_pads[port].connected = 1;
                        g_pads[port].device_slot = slot;
                        char buf[128];
                        snprintf(buf, sizeof(buf), "Assigned evdev slot %u to player port %u", slot, port);
                        log_msg(ASTRO_LOG_INFO, buf);
                    }
                    break;
                }
            }
        }
    }
    
    void* evdev_thread_main(void* arg) {
        (void)arg;
        std::vector<struct pollfd> pfds;
        for (const auto& dev : g_devices) {
            pfds.push_back({dev.fd, POLLIN, 0});
        }
        
        while (!g_stop.load(std::memory_order_relaxed)) {
            int timeout = (g_cfg.ppoll_timeout_ms == 0) ? -1 : (int)g_cfg.ppoll_timeout_ms;
            
            struct timespec ts_start;
            clock_gettime(CLOCK_MONOTONIC, &ts_start);
            
            int ret = poll(pfds.data(), pfds.size(), timeout);
            
            struct timespec ts_wake;
            clock_gettime(CLOCK_MONOTONIC, &ts_wake);
            
            uint64_t start_us = ts_start.tv_sec * 1000000ULL + ts_start.tv_nsec / 1000;
            uint64_t wake_us = ts_wake.tv_sec * 1000000ULL + ts_wake.tv_nsec / 1000;
            g_stats.last_wake_latency_us = (uint32_t)(wake_us - start_us);
            
            if (ret < 0) {
                if (errno == EINTR) continue;
                break;
            }
            if (ret == 0) continue; // timeout
            
            for (size_t i = 0; i < pfds.size(); ++i) {
                if (pfds[i].revents & POLLIN) {
                    struct input_event evs[64];
                    ssize_t bytes = read(pfds[i].fd, evs, sizeof(evs));
                    if (bytes > 0) {
                        size_t count = bytes / sizeof(struct input_event);
                        for (size_t j = 0; j < count; ++j) {
                            astro_input_event aie;
                            aie.ts_mono_us = evs[j].time.tv_sec * 1000000ULL + evs[j].time.tv_usec;
                            aie.seq = 0;
                            aie.device_slot = g_devices[i].slot;
                            aie.type = evs[j].type;
                            aie.code = evs[j].code;
                            aie.value = evs[j].value;
                            
                            assign_port_if_needed(aie.device_slot, aie.type, aie.value);
                            aie.port = g_devices[i].assigned_port;
                            
                            if (aie.port != 0xFFFF) {
                                if (g_ring.try_push(aie)) {
                                    __atomic_fetch_add(&g_stats.events_total, 1, __ATOMIC_RELAXED);
                                }
                            }
                        }
                    } else if (bytes < 0 && errno != EAGAIN && errno != EWOULDBLOCK) {
                        __atomic_fetch_add(&g_stats.disconnect_errors, 1, __ATOMIC_RELAXED);
                        close(pfds[i].fd);
                        pfds[i].fd = -1;
                    }
                }
            }
        }
        return nullptr;
    }
}

extern "C" {

int astro_input_start(const struct astro_input_config* cfg) {
    if (!cfg) return -1;
    g_cfg = *cfg;
    
    if (g_cfg.mlockall_enable) {
        if (mlockall(MCL_CURRENT | MCL_FUTURE) != 0) {
            log_msg(ASTRO_LOG_ERROR, "Failed to mlockall()");
        }
    }
    
    g_devices.clear();
    g_next_available_port.store(0);
    memset(g_pads, 0, sizeof(g_pads));
    memset(&g_stats, 0, sizeof(g_stats));
    
    char path[256];
    uint16_t slot = 0;
    for (int i = 0; i < 32 && slot < ASTRO_INPUT_MAX_DEVICES; ++i) {
        snprintf(path, sizeof(path), "/dev/input/event%d", i);
        int fd = open(path, O_RDONLY | O_NONBLOCK);
        if (fd >= 0) {
            if (ioctl(fd, EVIOCGRAB, 1) == 0) {
                unsigned int clockid = CLOCK_MONOTONIC;
                ioctl(fd, EVIOCSCLOCKID, &clockid);
                g_devices.push_back({fd, slot, 0xFFFF});
                slot++;
            } else {
                close(fd);
            }
        }
    }
    
    g_stats.devices_active = slot;
    g_stop.store(false, std::memory_order_relaxed);
    
    pthread_attr_t attr;
    pthread_attr_init(&attr);
    
    struct sched_param param;
    param.sched_priority = g_cfg.fifo_priority;
    pthread_attr_setschedpolicy(&attr, SCHED_FIFO);
    pthread_attr_setschedparam(&attr, &param);
    pthread_attr_setinheritsched(&attr, PTHREAD_EXPLICIT_SCHED);
    
    cpu_set_t cpuset;
    CPU_ZERO(&cpuset);
    for (int c = g_cfg.pin_cpu_first; c <= g_cfg.pin_cpu_last; ++c) {
        CPU_SET(c, &cpuset);
    }
    pthread_attr_setaffinity_np(&attr, sizeof(cpu_set_t), &cpuset);
    
    if (pthread_create(&g_thread_id, &attr, evdev_thread_main, nullptr) != 0) {
        log_msg(ASTRO_LOG_ERROR, "Failed to create RT thread with SCHED_FIFO, falling back");
        pthread_create(&g_thread_id, nullptr, evdev_thread_main, nullptr);
    }
    pthread_attr_destroy(&attr);
    
    return 0;
}

void astro_input_stop(void) {
    g_stop.store(true, std::memory_order_relaxed);
    pthread_join(g_thread_id, nullptr);
    for (auto& dev : g_devices) {
        if (dev.fd >= 0) {
            ioctl(dev.fd, EVIOCGRAB, 0);
            close(dev.fd);
        }
    }
    g_devices.clear();
}

static int map_btn_code(uint16_t code) {
    switch(code) {
        case BTN_EAST: return ASTRO_BTN_B;
        case BTN_SOUTH: return ASTRO_BTN_A;
        case BTN_WEST: return ASTRO_BTN_Y;
        case BTN_NORTH: return ASTRO_BTN_X;
        case BTN_START: return ASTRO_BTN_START;
        case BTN_SELECT: return ASTRO_BTN_SELECT;
        case BTN_DPAD_UP: return ASTRO_BTN_UP;
        case BTN_DPAD_DOWN: return ASTRO_BTN_DOWN;
        case BTN_DPAD_LEFT: return ASTRO_BTN_LEFT;
        case BTN_DPAD_RIGHT: return ASTRO_BTN_RIGHT;
        case BTN_TL: return ASTRO_BTN_L;
        case BTN_TR: return ASTRO_BTN_R;
        case BTN_TL2: return ASTRO_BTN_L2;
        case BTN_TR2: return ASTRO_BTN_R2;
        case BTN_THUMBL: return ASTRO_BTN_L3;
        case BTN_THUMBR: return ASTRO_BTN_R3;
        default: return -1;
    }
}

static int map_axis_code(uint16_t code) {
    switch(code) {
        case ABS_X: return 0;
        case ABS_Y: return 1;
        case ABS_Z: return 2;
        case ABS_RX: return 3;
        case ABS_RY: return 4;
        case ABS_RZ: return 5;
        default: return -1;
    }
}

unsigned astro_input_drain(struct astro_input_event* out, unsigned max_out) {
    unsigned n = g_ring.drain(out, max_out);
    for (unsigned i = 0; i < n; ++i) {
        uint16_t port = out[i].port;
        if (port < ASTRO_INPUT_MAX_PORTS) {
            if (out[i].type == EV_KEY) {
                int bit = map_btn_code(out[i].code);
                if (bit >= 0) {
                    if (out[i].value) {
                        g_pads[port].buttons |= (1U << bit);
                    } else {
                        g_pads[port].buttons &= ~(1U << bit);
                    }
                }
            } else if (out[i].type == EV_ABS) {
                int axis = map_axis_code(out[i].code);
                if (axis >= 0 && axis < ASTRO_INPUT_MAX_AXES) {
                    g_pads[port].axes[axis] = (int16_t)out[i].value;
                }
            }
        }
    }
    return n;
}

unsigned astro_input_poll_frame(struct astro_pad_state* pads, unsigned max_ports) {
    struct astro_input_event evs[ASTRO_INPUT_DRAIN_MAX];
    astro_input_drain(evs, ASTRO_INPUT_DRAIN_MAX);
    unsigned n = std::min(max_ports, (unsigned)ASTRO_INPUT_MAX_PORTS);
    for (unsigned i = 0; i < n; ++i) {
        pads[i] = g_pads[i];
    }
    return n;
}

int astro_input_button(unsigned port, unsigned button_bit) {
    if (port >= ASTRO_INPUT_MAX_PORTS) return 0;
    return (g_pads[port].buttons & (1U << button_bit)) ? 1 : 0;
}

int16_t astro_input_axis(unsigned port, unsigned axis) {
    if (port >= ASTRO_INPUT_MAX_PORTS || axis >= ASTRO_INPUT_MAX_AXES) return 0;
    return g_pads[port].axes[axis];
}

void astro_input_get_stats(struct astro_input_stats* out) {
    if (out) {
        *out = g_stats;
        out->events_dropped = g_ring.producer_dropped();
    }
}

const char* astro_input_ident(void) {
    return "astro_rt_v1";
}

} // extern "C"
