// =============================================================================
// B.E.E.R.C.A.D.E. A.S.T.R.O. - Lock-free SPSC Event Ring Buffer
// -----------------------------------------------------------------------------
// Single-Producer / Single-Consumer circular buffer for the 1000Hz sub-frame
// input pipeline. The producer is the SCHED_FIFO evdev RT thread (Cortex-A55,
// CPUs 0-3); the consumer is the RetroArch main frame loop draining events
// immediately before VBLANK.
//
// Ordering contract (C++20 / Itanium ARMv8.2-A):
//   * Producer: relaxed store of payload, then release store of head_.
//   * Consumer: acquire load of head_, read payload, relaxed store of tail_.
//   * Producer observes tail_ with acquire to safely reuse slots.
//
// Overflow policy: the producer NEVER blocks (a blocked RT thread is worse
// than a dropped event). If the ring is full the newest event is dropped and
// the `dropped` statistic is bumped (relaxed). Capacity is a compile-time
// power of two; index masking avoids a modulo.
//
// Zero dynamic allocation: storage is inline POD. All state is written with
// explicit memory orders; no fences, no atomics::wait, no syscalls.
// =============================================================================
#pragma once

#include <atomic>
#include <cstdint>
#include <cstddef>
#include <type_traits>

namespace astro_rt {

template <typename T, std::size_t Capacity>
class SpscRing {
    static_assert((Capacity & (Capacity - 1)) == 0 && Capacity != 0,
                  "Capacity must be a non-zero power of two");
    static_assert(std::is_trivially_copyable_v<T>,
                  "Astro RT ring payload must be trivially copyable (no ctors in hot path)");

    // Cache-line (and then some: RK3588 A55/A76 both use 64B lines) isolation
    // so producer and consumer indices never share a line (false sharing).
    struct alignas(64) CacheLine { std::byte pad[64]; };

public:
    constexpr SpscRing() = default;

    // --- Producer side (RT evdev thread) ------------------------------------

    // Attempt to publish one event. Returns false (and drops it) if full.
    // Wait-free: exactly one CAS-free pass, no loops, no syscalls.
    bool try_push(const T& item) noexcept {
        const std::uint32_t head = producer_.head.load(std::memory_order_relaxed);
        const std::uint32_t tail = consumer_.tail.load(std::memory_order_acquire);

        if (head - tail >= Capacity) {              // ring full -> drop newest
            dropped_.fetch_add(1, std::memory_order_relaxed);
            return false;
        }

        slots_[head & kMask] = item;                // plain (relaxed) payload store
        producer_.head.store(head + 1, std::memory_order_release); // publish
        return true;
    }

    // Producer-side peek of drop statistics (telemetry, non-hot-path).
    [[nodiscard]] std::uint32_t producer_dropped() const noexcept {
        return dropped_.load(std::memory_order_relaxed);
    }

    // --- Consumer side (RetroArch frame loop, pre-VBLANK) -------------------

    // Number of events available right now.
    [[nodiscard]] std::uint32_t size_approx() const noexcept {
        const std::uint32_t head = producer_.head.load(std::memory_order_acquire);
        const std::uint32_t tail = consumer_.tail.load(std::memory_order_relaxed);
        return head - tail;
    }

    // Pop one event into `out`. Returns false when the ring is empty.
    bool try_pop(T& out) noexcept {
        const std::uint32_t tail = consumer_.tail.load(std::memory_order_relaxed);
        const std::uint32_t head = producer_.head.load(std::memory_order_acquire);

        if (tail == head)                           // ring empty
            return false;

        out = slots_[tail & kMask];
        consumer_.tail.store(tail + 1, std::memory_order_release);
        return true;
    }

    // Drain up to `max_out` events; returns the number actually moved.
    // Used by the RetroArch hook to sweep every queued transition per frame.
    std::uint32_t drain(T* out, std::uint32_t max_out) noexcept {
        std::uint32_t n = 0;
        while (n < max_out && try_pop(out[n])) ++n;
        return n;
    }

    [[nodiscard]] static constexpr std::uint32_t capacity() noexcept { return Capacity; }

private:
    static constexpr std::uint32_t kMask = Capacity - 1;

    struct alignas(64) ProducerState {
        std::atomic<std::uint32_t> head{0};
    };
    
    struct alignas(64) ConsumerState {
        std::atomic<std::uint32_t> tail{0};
    };

    ProducerState producer_;
    ConsumerState consumer_;
    alignas(64) T slots_[Capacity]{};
    alignas(64) std::atomic<std::uint32_t> dropped_{0};

    static_assert(alignof(ProducerState) == 64, "ProducerState must be exactly 64-byte aligned to prevent false sharing");
    static_assert(alignof(ConsumerState) == 64, "ConsumerState must be exactly 64-byte aligned to prevent false sharing");
    static_assert(sizeof(ProducerState) == 64, "ProducerState struct size must be padded to a full cache line");
    static_assert(sizeof(ConsumerState) == 64, "ConsumerState struct size must be padded to a full cache line");
};

} // namespace astro_rt
