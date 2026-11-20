// =============================================================================
// B.E.E.R.C.A.D.E. A.S.T.R.O. - 1000Hz Evdev Sub-Frame Input Driver (public C ABI)
// -----------------------------------------------------------------------------
// Consumed by the RetroArch main frame loop just before VBLANK (frame_delay
// tail) through astro_input_drain() / astro_input_pad_state().
//
// Threading model:
//   * exactly one internal SCHED_FIFO(90) producer thread pinned to CPU 0-3
//   * the caller's thread is the sole consumer (SPSC)
//
// Allocation policy: astro_input_start() performs all setup allocation from
// its own preallocated static pools; after it returns, no function in this
// module allocates, locks, or makes a syscall other than ppoll/read/close.
// =============================================================================
#ifndef ASTRO_RT_INPUT_EVENTS_H
#define ASTRO_RT_INPUT_EVENTS_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define ASTRO_INPUT_API_VERSION     1
#define ASTRO_INPUT_MAX_DEVICES     16   /* preallocated evdev slots           */
#define ASTRO_INPUT_MAX_PORTS       4    /* player ports (P1..P4)              */
#define ASTRO_INPUT_MAX_AXES        8    /* abs axes tracked per device        */
#define ASTRO_INPUT_DRAIN_MAX       256  /* per-frame drain batch limit        */

#define ASTRO_RT_STATS_SHM_NAME     "/astro_rt_stats"
#define ASTRO_RT_STATS_MAGIC        0x415354524F525431ULL /* "ASTRORT1" */
#define ASTRO_RT_STATS_VERSION      1u
#define ASTRO_RT_HIST_BUCKETS       32

/* libretro-compatible button bit positions (RETRO_DEVICE_ID_JOYPAD_*) */
enum astro_pad_buttons {
    ASTRO_BTN_B      = 0,  ASTRO_BTN_Y      = 1,  ASTRO_BTN_SELECT = 2,
    ASTRO_BTN_START  = 3,  ASTRO_BTN_UP     = 4,  ASTRO_BTN_DOWN   = 5,
    ASTRO_BTN_LEFT   = 6,  ASTRO_BTN_RIGHT  = 7,  ASTRO_BTN_A      = 8,
    ASTRO_BTN_X      = 9,  ASTRO_BTN_L      = 10, ASTRO_BTN_R      = 11,
    ASTRO_BTN_L2     = 12, ASTRO_BTN_R2     = 13, ASTRO_BTN_L3     = 14,
    ASTRO_BTN_R3     = 15
};
#define ASTRO_BTN_MASK_ALL 0xFFFFu

/* Aggregated pad snapshot: what input_state() answers from, O(1), lock-free. */
struct astro_pad_state {
    uint32_t buttons;               /* bitmask of astro_pad_buttons        */
    int16_t  axes[ASTRO_INPUT_MAX_AXES];  /* -32768..32767 (abs scaled)    */
    uint16_t device_slot;           /* backing evdev slot (diagnostics)    */
    uint16_t connected;
};

/* One timestamped transition. ts_mono_us comes from the kernel evdev stamp
 * (EVIOCSCLOCKID -> CLOCK_MONOTONIC), i.e. sub-frame microsecond precision
 * measured at the encoder, before any scheduler wake-up jitter. */
struct astro_input_event {
    uint64_t ts_mono_us;    /* kernel event timestamp, us since boot      */
    uint32_t seq;           /* monotonic producer sequence (diagnostics)  */
    uint16_t device_slot;   /* evdev slot index [0, MAX_DEVICES)          */
    uint16_t port;          /* mapped player port [0, MAX_PORTS)          */
    uint16_t type;          /* struct input_event.type (EV_KEY/EV_ABS)    */
    uint16_t code;          /* BTN_EAST / ABS_X / ... (linux-event-codes) */
    int32_t  value;         /* transition value                           */
};

enum astro_input_log_level { ASTRO_LOG_SILENT = 0, ASTRO_LOG_ERROR = 1, ASTRO_LOG_INFO = 2 };

struct astro_input_config {
    uint8_t  fifo_priority;      /* SCHED_FIFO priority, 90 per spec       */
    uint8_t  pin_cpu_first;      /* first A55 core (0)                     */
    uint8_t  pin_cpu_last;       /* last  A55 core (3)                     */
    uint8_t  mlockall_enable;    /* lock pages to defeat page-fault jitter */
    uint32_t ppoll_timeout_ms;   /* watchdog tick, 0 -> infinite block     */
    enum astro_input_log_level log_level;
};

/* Statistics for telemetry push to B.O.S.S. via astro-daemon. All counters
 * are relaxed atomics on the producer side; safe to read anytime. */
struct astro_input_stats {
    uint32_t events_total;       /* transitions accepted into the ring     */
    uint32_t events_dropped;     /* ring-full drops (should stay 0)        */
    uint32_t hotplug_adds;       /* devices attached at runtime            */
    uint32_t hotplug_removes;    /* devices detached at runtime            */
    uint32_t disconnect_errors;  /* ENODEV/EIO read failures recovered     */
    uint32_t devices_active;     /* currently open evdev fds               */
    uint32_t last_wake_latency_us; /* ppoll wake -> last event handled     */
};

/* POSIX shared-memory telemetry snapshot at /dev/shm/astro_rt_stats
 * (ASTRO_RT_STATS_SHM_NAME), read lock-free by astro-daemon.
 *
 * Fixed 184-byte ABI layout (zero implicit padding):
 *   offset   0 (8B): magic           = ASTRO_RT_STATS_MAGIC
 *   offset   8 (4B): version         = ASTRO_RT_STATS_VERSION
 *   offset  12 (4B): seq             = seqlock (odd = write active, even = stable)
 *   offset  16 (4B): poll_rate_hz    = nominal polling loop rate in Hz
 *   offset  20 (4B): p50_us          = 50th percentile wake-to-event latency (us)
 *   offset  24 (4B): p95_us          = 95th percentile wake-to-event latency (us)
 *   offset  28 (4B): p99_us          = 99th percentile wake-to-event latency (us)
 *   offset  32 (4B): max_us          = maximum wake-to-event latency observed (us)
 *   offset  36 (4B): events_total    = transitions accepted into the SPSC ring
 *   offset  40 (4B): events_dropped  = SPSC ring-full drops
 *   offset  44 (4B): devices_active  = currently open evdev fds
 *   offset  48 (4B): hotplug_adds    = cumulative runtime inotify adds
 *   offset  52 (4B): hotplug_removes = cumulative runtime device removals
 *   offset  56 (128B): buckets[32]   = log2 latency histogram in microseconds:
 *                                      bucket 0 = [0, 1] us,
 *                                      bucket b = [1<<b, (1<<(b+1))-1] us
 */
struct astro_input_shm_stats {
    uint64_t magic;
    uint32_t version;
    uint32_t seq;
    uint32_t poll_rate_hz;
    uint32_t p50_us;
    uint32_t p95_us;
    uint32_t p99_us;
    uint32_t max_us;
    uint32_t events_total;
    uint32_t events_dropped;
    uint32_t devices_active;
    uint32_t hotplug_adds;
    uint32_t hotplug_removes;
    uint32_t buckets[ASTRO_RT_HIST_BUCKETS];
};

/* ---- lifecycle ---------------------------------------------------------- */

/* Bring up the RT polling thread and enumerate existing /dev/input/event*.
 * Returns 0 on success, negative errno-style code on failure. Idempotent. */
int astro_input_start(const struct astro_input_config* cfg);

/* Stop the RT thread and close all devices. Never blocks indefinitely. */
void astro_input_stop(void);

/* ---- consumer hooks (RetroArch frame loop, pre-VBLANK) ------------------- */

/* Drain queued transitions into `out` (max ASTRO_INPUT_DRAIN_MAX), applying
 * them to the internal pad snapshot. Returns the number drained. Call once
 * per frame immediately before the swap / VBLANK wait. */
unsigned astro_input_drain(struct astro_input_event* out, unsigned max_out);

/* Drain and additionally copy out the aggregated per-port snapshot. */
unsigned astro_input_poll_frame(struct astro_pad_state* pads, unsigned max_ports);

/* O(1) current-state query for input_state(): port + button bit -> 0/1. */
int astro_input_button(unsigned port, unsigned button_bit);

/* O(1) current axis query; returns int16 in [-32768, 32767]. */
int16_t astro_input_axis(unsigned port, unsigned axis);

/* ---- telemetry ----------------------------------------------------------- */

void astro_input_get_stats(struct astro_input_stats* out);

/* Scale raw EV_ABS value from [minimum, maximum] linearly into [-32768, 32767].
 * Returns 0 when maximum <= minimum; clamps out-of-range inputs cleanly. */
int16_t astro_input_scale_abs(int32_t value, int32_t minimum, int32_t maximum);

/* Compute p50, p95, and p99 microsecond latency values from log2 buckets where
 * bucket 0 covers [0, 1] us and bucket b covers [1<<b, (1<<(b+1))-1] us. */
void astro_input_compute_percentiles(const uint32_t* buckets,
                                     unsigned num_buckets,
                                     uint32_t* p50_us,
                                     uint32_t* p95_us,
                                     uint32_t* p99_us);

/* Library identity, for RetroArch driver registration asserts. */
const char* astro_input_ident(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* ASTRO_RT_INPUT_EVENTS_H */
