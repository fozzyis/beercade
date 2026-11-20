#include "astro_rt/input_events.h"
#include "astro_rt/spsc_ring.hpp"

#include <atomic>
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <thread>
#include <vector>

namespace {

astro_input_event make_event(std::uint64_t ts, std::uint16_t code, std::int32_t value) {
    astro_input_event ev{};
    ev.ts_mono_us = ts;
    ev.seq = static_cast<std::uint32_t>(ts);
    ev.device_slot = 0;
    ev.port = 0;
    ev.type = 1;
    ev.code = code;
    ev.value = value;
    return ev;
}

void test_empty_and_capacity() {
    astro_rt::SpscRing<astro_input_event, 64> ring;
    assert(ring.capacity() == 64);
    assert(ring.size_approx() == 0);
    assert(ring.producer_dropped() == 0);

    astro_input_event out{};
    assert(!ring.try_pop(out));
    assert(ring.drain(&out, 1) == 0);
}

void test_fifo_and_wraparound() {
    astro_rt::SpscRing<astro_input_event, 64> ring;
    for (std::uint64_t round = 0; round < 4; ++round) {
        for (std::uint64_t i = 0; i < 40; ++i) {
            const std::uint64_t seq = round * 40 + i;
            assert(ring.try_push(make_event(seq, static_cast<std::uint16_t>(seq & 0xffffu), 1)));
        }
        assert(ring.size_approx() == 40);
        for (std::uint64_t i = 0; i < 40; ++i) {
            const std::uint64_t seq = round * 40 + i;
            astro_input_event ev{};
            assert(ring.try_pop(ev));
            assert(ev.ts_mono_us == seq);
            assert(ev.code == static_cast<std::uint16_t>(seq & 0xffffu));
        }
        assert(ring.size_approx() == 0);
    }
}

void test_overflow_drops_newest() {
    astro_rt::SpscRing<astro_input_event, 64> ring;
    for (std::uint64_t i = 0; i < 64; ++i) {
        assert(ring.try_push(make_event(1000 + i, 10, 1)));
    }
    assert(ring.size_approx() == 64);
    assert(!ring.try_push(make_event(9999, 10, 1)));
    assert(ring.producer_dropped() == 1);

    std::vector<astro_input_event> batch(64);
    const std::uint32_t drained = ring.drain(batch.data(), static_cast<std::uint32_t>(batch.size()));
    assert(drained == 64);
    for (std::uint32_t i = 0; i < drained; ++i) {
        assert(batch[i].ts_mono_us == 1000 + i);
    }
    assert(ring.size_approx() == 0);
}

void test_drain_partial() {
    astro_rt::SpscRing<astro_input_event, 64> ring;
    for (std::uint64_t i = 0; i < 10; ++i) {
        assert(ring.try_push(make_event(i, 1, static_cast<std::int32_t>(i))));
    }
    astro_input_event first[4]{};
    assert(ring.drain(first, 4) == 4);
    for (std::uint32_t i = 0; i < 4; ++i) {
        assert(first[i].ts_mono_us == i);
    }
    assert(ring.size_approx() == 6);

    astro_input_event rest[8]{};
    assert(ring.drain(rest, 8) == 6);
    for (std::uint32_t i = 0; i < 6; ++i) {
        assert(rest[i].ts_mono_us == i + 4);
    }
    assert(ring.size_approx() == 0);
}

void test_spsc_concurrency() {
    constexpr std::uint64_t kTotal = 100000;
    astro_rt::SpscRing<astro_input_event, 1024> ring;
    std::atomic<bool> producer_done{false};
    std::uint64_t popped = 0;
    std::uint64_t last_ts = 0;
    bool first = true;

    std::thread producer([&]() {
        for (std::uint64_t seq = 1; seq <= kTotal; ++seq) {
            if (!ring.try_push(make_event(seq, static_cast<std::uint16_t>(seq & 0xffu), 1))) {
                std::this_thread::yield();
            }
        }
        producer_done.store(true, std::memory_order_release);
    });

    std::thread consumer([&]() {
        astro_input_event batch[64]{};
        while (!producer_done.load(std::memory_order_acquire) || ring.size_approx() > 0) {
            const std::uint32_t n = ring.drain(batch, 64);
            for (std::uint32_t i = 0; i < n; ++i) {
                if (!first) {
                    assert(batch[i].ts_mono_us > last_ts);
                }
                first = false;
                last_ts = batch[i].ts_mono_us;
                ++popped;
            }
            if (n == 0) {
                std::this_thread::yield();
            }
        }
    });

    producer.join();
    consumer.join();

    const std::uint64_t dropped = ring.producer_dropped();
    assert(popped + dropped == kTotal);
    assert(popped > 0);
}

} // namespace

int main() {
    test_empty_and_capacity();
    test_fifo_and_wraparound();
    test_overflow_drops_newest();
    test_drain_partial();
    test_spsc_concurrency();
    std::printf("astro_rt_spsc_test: all checks passed\n");
    return 0;
}
