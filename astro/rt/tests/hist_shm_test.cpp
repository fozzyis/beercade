#include "astro_rt/input_events.h"

#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

#include <cassert>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>

static_assert(sizeof(struct astro_input_shm_stats) == 184,
              "astro_input_shm_stats must be a tightly packed 184-byte ABI");
static_assert(offsetof(struct astro_input_shm_stats, magic) == 0, "magic offset");
static_assert(offsetof(struct astro_input_shm_stats, version) == 8, "version offset");
static_assert(offsetof(struct astro_input_shm_stats, seq) == 12, "seq offset");
static_assert(offsetof(struct astro_input_shm_stats, poll_rate_hz) == 16, "poll_rate_hz offset");
static_assert(offsetof(struct astro_input_shm_stats, p50_us) == 20, "p50_us offset");
static_assert(offsetof(struct astro_input_shm_stats, p95_us) == 24, "p95_us offset");
static_assert(offsetof(struct astro_input_shm_stats, p99_us) == 28, "p99_us offset");
static_assert(offsetof(struct astro_input_shm_stats, max_us) == 32, "max_us offset");
static_assert(offsetof(struct astro_input_shm_stats, events_total) == 36, "events_total offset");
static_assert(offsetof(struct astro_input_shm_stats, events_dropped) == 40, "events_dropped offset");
static_assert(offsetof(struct astro_input_shm_stats, devices_active) == 44, "devices_active offset");
static_assert(offsetof(struct astro_input_shm_stats, hotplug_adds) == 48, "hotplug_adds offset");
static_assert(offsetof(struct astro_input_shm_stats, hotplug_removes) == 52, "hotplug_removes offset");
static_assert(offsetof(struct astro_input_shm_stats, buckets) == 56, "buckets offset");

namespace {

void test_percentiles_empty_and_null() {
    uint32_t p50 = 999, p95 = 999, p99 = 999;
    astro_input_compute_percentiles(nullptr, 32, &p50, &p95, &p99);
    assert(p50 == 0 && p95 == 0 && p99 == 0);

    uint32_t zeros[ASTRO_RT_HIST_BUCKETS]{};
    p50 = p95 = p99 = 999;
    astro_input_compute_percentiles(zeros, ASTRO_RT_HIST_BUCKETS, &p50, &p95, &p99);
    assert(p50 == 0 && p95 == 0 && p99 == 0);
}

void test_percentiles_bucket_ranges() {
    uint32_t b0[ASTRO_RT_HIST_BUCKETS]{};
    b0[0] = 1;
    uint32_t p50 = 0, p95 = 0, p99 = 0;
    astro_input_compute_percentiles(b0, ASTRO_RT_HIST_BUCKETS, &p50, &p95, &p99);
    assert(p50 <= 1 && p95 <= 1 && p99 <= 1);
    assert(p99 == 1);

    // 90 samples in [64, 127], 6 in [256, 511], 4 in [1024, 2047]
    uint32_t dist[ASTRO_RT_HIST_BUCKETS]{};
    dist[6] = 90;
    dist[8] = 6;
    dist[10] = 4;
    astro_input_compute_percentiles(dist, ASTRO_RT_HIST_BUCKETS, &p50, &p95, &p99);
    assert(p50 >= 64 && p50 <= 127);
    assert(p95 >= 256 && p95 <= 511);
    assert(p99 >= 1024 && p99 <= 2047);
    assert(p50 <= p95 && p95 <= p99);

    // Bucket 31 boundary: [2^31, UINT32_MAX] must not shift-overflow
    uint32_t high[ASTRO_RT_HIST_BUCKETS]{};
    high[31] = 10;
    astro_input_compute_percentiles(high, ASTRO_RT_HIST_BUCKETS, &p50, &p95, &p99);
    assert(p50 >= (1U << 31));
    assert(p50 <= p95 && p95 <= p99);
}

void test_shm_lifecycle_and_readback() {
    assert(std::strcmp(astro_input_ident(), "astro_rt_v1") == 0);

    astro_input_config cfg{};
    cfg.fifo_priority = 0;
    cfg.pin_cpu_first = 0;
    cfg.pin_cpu_last = 0;
    cfg.mlockall_enable = 0;
    cfg.ppoll_timeout_ms = 0; // Also exercises non-blocking stop wakeup
    cfg.log_level = ASTRO_LOG_SILENT;

    assert(astro_input_start(&cfg) == 0);
    assert(astro_input_start(&cfg) == 0); // idempotent

    const int fd = shm_open(ASTRO_RT_STATS_SHM_NAME, O_RDONLY, 0);
    assert(fd >= 0);

    struct stat st{};
    assert(fstat(fd, &st) == 0);
    assert(static_cast<size_t>(st.st_size) >= sizeof(astro_input_shm_stats));

    void* ptr = mmap(nullptr, sizeof(astro_input_shm_stats), PROT_READ, MAP_SHARED, fd, 0);
    assert(ptr != MAP_FAILED);

    const auto* shm = static_cast<const astro_input_shm_stats*>(ptr);
    astro_input_shm_stats snap{};
    for (int attempt = 0; attempt < 16; ++attempt) {
        const uint32_t s1 = __atomic_load_n(&shm->seq, __ATOMIC_ACQUIRE);
        if (s1 & 1u) {
            continue;
        }
        std::memcpy(&snap, shm, sizeof(snap));
        __atomic_thread_fence(__ATOMIC_ACQUIRE);
        const uint32_t s2 = __atomic_load_n(&shm->seq, __ATOMIC_ACQUIRE);
        if (s1 == s2) {
            break;
        }
    }

    assert(snap.magic == ASTRO_RT_STATS_MAGIC);
    assert(snap.version == ASTRO_RT_STATS_VERSION);
    assert((snap.seq & 1u) == 0u);
    assert(snap.poll_rate_hz == 1000u);

    astro_input_stats stats{};
    astro_input_get_stats(&stats);
    assert(stats.events_dropped == snap.events_dropped);

    assert(munmap(ptr, sizeof(astro_input_shm_stats)) == 0);
    assert(close(fd) == 0);

    astro_input_stop();
    astro_input_stop(); // idempotent

    assert(shm_unlink(ASTRO_RT_STATS_SHM_NAME) == 0);
}

} // namespace

int main() {
    test_percentiles_empty_and_null();
    test_percentiles_bucket_ranges();
    test_shm_lifecycle_and_readback();
    std::printf("astro_rt_hist_shm_test: all checks passed\n");
    return 0;
}
