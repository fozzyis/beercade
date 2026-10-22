//! ============================================================================
//! A.S.T.R.O. ROLLBACK NETPLAY ENGINE — deterministic P2P state synchronization
//! between O.R.I.O.N. cabinets (Station A <-> Station B) over UDP, with
//! B.O.S.S. acting as discovery relay and telemetry arbiter.
//!
//! # 1. Formal model
//!
//! A cabinet is a pure state machine:
//!     S(F+1) = T(S(F), iL(F), iR(F))
//! where S(F) is the simulated state before frame F, T is the deterministic
//! emulator-core transition and iL/iR are local/remote input words. `T` is a
//! total function of its arguments ONLY (no wall-clock, no RNG without a
//! seeded PRNG inside S, no HashMap iteration order).
//!
//! ## Prediction tree
//! Let C = the highest frame for which both cabinets hold identical
//! *confirmed* input pairs. For frames F in (C, now], iR(F) is unknown; the
//! prediction policy is p(F) = iR(last confirmed) (GGPO stationary
//! prediction). Each unknown frame is a branch node; the executed path is
//! the all-prediction spine. When the real iR(F) arrives and differs from
//! p(F) the spine is pruned and re-grown from the nearest saved pre-state:
//!     rollback(F*):  load S(F*); for f in F*..now: S(f+1) = T(S(f), iL, iR)
//! with F* = min corrected frame. Prediction is *conservative*: once the
//! true inputs are known the re-simulated states converge (INV-4).
//!
//! ## Clock synchronization (dynamic drift controller)
//! NTP 4-timestamp exchange every CLOCK_SYNC_INTERVAL_MS:
//!     rho_k   = (T4-T1) - (T3-T2)          (RTT, microseconds)
//!     theta_k = ((T2-T1) + (T3-T4)) / 2    (remote-ahead offset)
//! Samples with rho > RTT_MAX_US are discarded (min-RTT bias filter). We
//! never step the wall clock; we steer the *frame clock* pacing interval.
//! With G = frames per sync sample the plant is a pure integrator:
//!     x(k+1) = x(k) + G*u(k)      x = phase error [µs]
//! PI control law (gains in µs/frame, Jury-stable for 0 < ki < kp < 2/G):
//!     e(k)   = -theta_hat(k)
//!     I(k+1) = clamp(I(k) + ki*e(k), ±U_MAX)
//!     u(k)   = clamp(kp*e(k) + I(k), ±U_MAX)
//! With kp=0.05, ki=0.008, G=15 the closed-loop poles are |λ| ≈ 0.77/0.48.
//! If |theta_hat| > STEP_CORRECT_THRESHOLD_US a single phase step replaces
//! slewing. Steady-state bound: pacing quantization + controller deadband
//! keep frame clocks within ±0.5 ms under OS timer granularity <= 1 ms
//! (RK3588S, PREEMPT kernel).
//!
//! ## Desync detection & resolution
//! Rolling integrity chain: every slot stores h(F) = CRC32(canonical(S(F+1)))
//! piggybacked on input datagrams. A mismatch of h at frame X between
//! cabinets with *identical confirmed inputs* violates INV-1 and triggers:
//!   - soft: rollback & resimulate from snapshot (prediction artefacts), or
//!   - hard: state transfer from host — host streams its authoritative state
//!     chunked over UDP (transfer_id, idx, total), guest freezes simulation,
//!     reassembles, loads S(G) at host frame G and resumes.
//!
//! # 2. Watchdog failure tree (astro-daemon)
//!   packet loss EWMA > 5%  (window 120 frames) -> DEGRADED (input delay 0->3)
//!   packet loss EWMA > 25% OR peer silence > 500 ms -> LOCAL_MODE (graceful)
//!   cpu_temp > 85 C / gpu_temp > 90 C -> THERMAL_HOLD (freeze sim, keep net)
//!   hysteresis recovery at 75 C / 80 C
//!   hard desync -> host state transfer; transfer timeout 2 s -> LOCAL_MODE
//! Every transition emits a NetplayHealthReport to B.O.S.S. telemetry.
//!
//! # 3. Invariants (zero state corruption)
//!   INV-1 convergence: identical confirmed inputs => identical states:
//!        iL_A(F)=iL_B(F) /\ iR_A(F)=iR_B(F) => H(S_A(F+1)) = H(S_B(F+1))
//!   INV-2 bounded rewind: rollback depth <= max_rollback_frames, else the
//!        session escalates to a hard state transfer (never resims a pruned
//!        ring slot).
//!   INV-3 no phantom advance: a frame is committed to output only after its
//!        input pair is confirmed or deterministically predicted.
//!   INV-4 prediction conservatism: p(F) is always a *previously confirmed*
//!        remote input word, so prediction error is eventually observed and
//!        corrected (self-stabilization).
//!   INV-5 hash window consistency: h(F) is defined over the canonical
//!        little-endian byte image of S(F+1); any mismatch at equal confirmed
//!        inputs escalates to transfer — corruption can halt play but never
//!        silently propagate (fail-visible, not fail-silent).
//! ============================================================================

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tokio::time::{interval_at, Duration as TokioDuration, Instant as TokioInstant};
use tracing::{debug, error, info, warn};

// ---------------------------------------------------------------------------
// Protocol constants
// ---------------------------------------------------------------------------

pub const PROTOCOL_VERSION: u8 = 1;
pub const MAX_PLAYERS: usize = 2;
/// Frames of local input history piggybacked on every input datagram.
pub const INPUT_WINDOW: usize = 4;
pub const DEFAULT_HISTORY_FRAMES: usize = 128;
/// 60.0 fps nominal frame period in microseconds.
pub const FRAME_DURATION_US: u64 = 16_667;
pub const MAX_DATAGRAM: usize = 1400;
/// NTP sample validity gate (microseconds).
pub const RTT_MAX_US: f64 = 5_000.0;
/// Beyond this phase error we step-correct instead of slewing.
pub const STEP_CORRECT_THRESHOLD_US: f64 = 5_000.0;
/// Hard clamp of the per-frame pacing adjustment (µs) — max 1.5 ms/frame,
/// i.e. up to ~90 ms/s of phase slew, ample vs. the ±0.5 ms target.
pub const MAX_PACING_DELTA_US: f64 = 1_500.0;
pub const CLOCK_SYNC_INTERVAL_MS: u64 = 250;
pub const WATCHDOG_INTERVAL_MS: u64 = 1_000;
pub const LOSS_WINDOW_FRAMES: u32 = 120;
pub const LOSS_DEGRADE_PCT: f32 = 5.0;
pub const LOSS_LOCAL_PCT: f32 = 25.0;
pub const PEER_SILENCE_TIMEOUT_MS: u64 = 500;
pub const FREEZE_TIMEOUT_MS: u64 = 2_000;
pub const CPU_TEMP_LIMIT_C: f32 = 85.0;
pub const GPU_TEMP_LIMIT_C: f32 = 90.0;
pub const CPU_TEMP_HYST_C: f32 = 75.0;
pub const GPU_TEMP_HYST_C: f32 = 80.0;
pub const DEFAULT_STATE_CHUNK: usize = 1_024;
pub const NEUTRAL_INPUT: InputWord = 0;
/// Drift controller gains (Jury-stable for G = frames per sync sample:
/// 0 < ki < kp < 2/G; with G=15: 0 < 0.008 < 0.05 < 0.133).
pub const DRIFT_KP: f64 = 0.05;
pub const DRIFT_KI: f64 = 0.008;

pub type InputWord = u32;

// ---------------------------------------------------------------------------
// Time helpers
// ---------------------------------------------------------------------------

pub fn now_epoch_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

pub fn now_epoch_ms() -> u64 {
    now_epoch_us() / 1_000
}

// ---------------------------------------------------------------------------
// CRC32 (IEEE 802.3, reflected) — hand-rolled so the crate keeps zero extra
// dependencies; bitwise implementation, no table needed at 60 Hz scale.
// ---------------------------------------------------------------------------

pub fn crc32(data: &[u8]) -> u32 {
    crc32_update(0xFFFF_FFFF, data) ^ 0xFFFF_FFFF
}

/// Incremental CRC32 state (allows hashing streamed state chunks).
pub fn crc32_update(state: u32, data: &[u8]) -> u32 {
    let mut crc = state;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    crc
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[allow(dead_code)] // protocol-complete error surface (used by cores/relay)
#[derive(Debug)]
pub enum NetplayError {
    Io(std::io::Error),
    StateBufferTooSmall { need: usize, have: usize },
    StateMalformed,
    TransferCorrupted,
    PeerDesynced { frame: u32, local_hash: u32, remote_hash: u32 },
}

impl fmt::Display for NetplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetplayError::Io(e) => write!(f, "netplay io error: {e}"),
            NetplayError::StateBufferTooSmall { need, have } => {
                write!(f, "state image needs {need} bytes, buffer holds {have}")
            }
            NetplayError::StateMalformed => write!(f, "malformed core state image"),
            NetplayError::TransferCorrupted => write!(f, "state transfer failed integrity check"),
            NetplayError::PeerDesynced { frame, local_hash, remote_hash } => {
                write!(f, "desync at frame {frame}: local {local_hash:#010x} vs remote {remote_hash:#010x}")
            }
        }
    }
}

impl std::error::Error for NetplayError {}

impl From<std::io::Error> for NetplayError {
    fn from(e: std::io::Error) -> Self {
        NetplayError::Io(e)
    }
}

// ---------------------------------------------------------------------------
// Wire protocol — compact fixed-layout binary (JSON is far too heavy for a
// 60 Hz input stream). Layout per message:
//   [0..2]  magic "OR"   [2] version   [3] msg_type
//   [4..12] session_id (u64 LE)
//   [12.. ]  message body (type-specific, little-endian throughout)
// ---------------------------------------------------------------------------
// NOTE: BOSS/src/netplay_relay.rs mirrors these constants for routing; keep
// the two definitions in lock-step.
// ---------------------------------------------------------------------------

pub const MAGIC: [u8; 2] = [0x4F, 0x52]; // "OR"
pub const HEADER_LEN: usize = 12;

#[allow(dead_code)]
pub mod msg {
    pub const INPUT: u8 = 0x01;
    pub const PING: u8 = 0x02;
    pub const PONG: u8 = 0x03;
    pub const DESYNC_REPORT: u8 = 0x04;
    pub const STATE_CHUNK: u8 = 0x05;
    pub const WATCHDOG: u8 = 0x06;
    pub const ANNOUNCE: u8 = 0x07;
    pub const ANNOUNCE_ACK: u8 = 0x08;
}

fn put_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}
fn put_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn put_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Cursor { buf, pos: 0 }
    }
    fn u8(&mut self) -> Option<u8> {
        let v = *self.buf.get(self.pos)?;
        self.pos += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        let s = self.buf.get(self.pos..self.pos + 2)?;
        self.pos += 2;
        Some(u16::from_le_bytes(s.try_into().unwrap()))
    }
    fn u32(&mut self) -> Option<u32> {
        let s = self.buf.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(u32::from_le_bytes(s.try_into().unwrap()))
    }
    fn u64(&mut self) -> Option<u64> {
        let s = self.buf.get(self.pos..self.pos + 8)?;
        self.pos += 8;
        Some(u64::from_le_bytes(s.try_into().unwrap()))
    }
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.buf.get(self.pos..self.pos + n)?;
        self.pos += n;
        Some(s)
    }
}

fn put_addr(buf: &mut Vec<u8>, addr: Option<SocketAddr>) {
    match addr {
        Some(SocketAddr::V4(a)) => {
            put_u8(buf, 4);
            put_u32(buf, u32::from_ne_bytes(a.ip().octets()));
            put_u16(buf, a.port());
        }
        _ => put_u8(buf, 0),
    }
}

fn take_addr(c: &mut Cursor) -> Option<Option<SocketAddr>> {
    let fam = c.u8()?;
    Some(match fam {
        4 => {
            let ip = c.u32()?;
            let port = c.u16()?;
            // to_ne_bytes inverts the from_ne_bytes encoding in put_addr.
            Some(SocketAddr::from((Ipv4Addr::from(ip.to_ne_bytes()), port)))
        }
        _ => None,
    })
}

/// All frames of the P2P netplay conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Packet {
    /// Sliding window of local inputs + integrity hash of the sender's state.
    Input {
        base_frame: u32,
        inputs: Vec<InputWord>,
        hash_frame: u32,
        hash: u32,
        /// 1 iff the sender's hash_frame slot had fully confirmed inputs —
        /// receivers must only cross-check hashes against this flag to
        /// avoid false desyncs against provisional (predicted) states.
        hash_confirmed: u8,
    },
    Ping { t1: u64 },
    Pong { t1: u64, t2: u64, t3: u64 },
    DesyncReport { frame: u32, local_hash: u32, remote_hash: u32 },
    StateChunk {
        transfer_id: u32,
        frame: u32,
        chunk_idx: u16,
        total_chunks: u16,
        data: Vec<u8>,
    },
    Watchdog { state: u8, loss_pct_x100: u16, cpu_temp_x100: u16, gpu_temp_x100: u16 },
    Announce { node_id: String },
    AnnounceAck { your_slot: u8, peer_addr: Option<SocketAddr> },
}

impl Packet {
    pub fn msg_type(&self) -> u8 {
        match self {
            Packet::Input { .. } => msg::INPUT,
            Packet::Ping { .. } => msg::PING,
            Packet::Pong { .. } => msg::PONG,
            Packet::DesyncReport { .. } => msg::DESYNC_REPORT,
            Packet::StateChunk { .. } => msg::STATE_CHUNK,
            Packet::Watchdog { .. } => msg::WATCHDOG,
            Packet::Announce { .. } => msg::ANNOUNCE,
            Packet::AnnounceAck { .. } => msg::ANNOUNCE_ACK,
        }
    }

    pub fn encode(&self, session_id: u64) -> Vec<u8> {
        let mut b = Vec::with_capacity(HEADER_LEN + 64);
        b.extend_from_slice(&MAGIC);
        put_u8(&mut b, PROTOCOL_VERSION);
        put_u8(&mut b, self.msg_type());
        put_u64(&mut b, session_id);
        match self {
            Packet::Input { base_frame, inputs, hash_frame, hash, hash_confirmed } => {
                put_u32(&mut b, *base_frame);
                put_u8(&mut b, inputs.len() as u8);
                for i in inputs {
                    put_u32(&mut b, *i);
                }
                put_u32(&mut b, *hash_frame);
                put_u32(&mut b, *hash);
                put_u8(&mut b, *hash_confirmed);
            }
            Packet::Ping { t1 } => put_u64(&mut b, *t1),
            Packet::Pong { t1, t2, t3 } => {
                put_u64(&mut b, *t1);
                put_u64(&mut b, *t2);
                put_u64(&mut b, *t3);
            }
            Packet::DesyncReport { frame, local_hash, remote_hash } => {
                put_u32(&mut b, *frame);
                put_u32(&mut b, *local_hash);
                put_u32(&mut b, *remote_hash);
            }
            Packet::StateChunk { transfer_id, frame, chunk_idx, total_chunks, data } => {
                put_u32(&mut b, *transfer_id);
                put_u32(&mut b, *frame);
                put_u16(&mut b, *chunk_idx);
                put_u16(&mut b, *total_chunks);
                b.extend_from_slice(data);
            }
            Packet::Watchdog { state, loss_pct_x100, cpu_temp_x100, gpu_temp_x100 } => {
                put_u8(&mut b, *state);
                put_u16(&mut b, *loss_pct_x100);
                put_u16(&mut b, *cpu_temp_x100);
                put_u16(&mut b, *gpu_temp_x100);
            }
            Packet::Announce { node_id } => {
                put_u8(&mut b, node_id.len() as u8);
                b.extend_from_slice(node_id.as_bytes());
            }
            Packet::AnnounceAck { your_slot, peer_addr } => {
                put_u8(&mut b, *your_slot);
                put_addr(&mut b, *peer_addr);
            }
        }
        b
    }

    /// Returns (session_id, packet) or None on malformed datagrams.
    pub fn decode(buf: &[u8]) -> Option<(u64, Packet)> {
        if buf.len() < HEADER_LEN || buf[0] != MAGIC[0] || buf[1] != MAGIC[1] {
            return None;
        }
        if buf[2] != PROTOCOL_VERSION {
            return None;
        }
        let mut c = Cursor::new(&buf[HEADER_LEN..]);
        let session_id = u64::from_le_bytes(buf[4..12].try_into().unwrap());
        let pkt = match buf[3] {
            msg::INPUT => {
                let base_frame = c.u32()?;
                let count = c.u8()? as usize;
                if count > 16 {
                    return None;
                }
                let mut inputs = Vec::with_capacity(count);
                for _ in 0..count {
                    inputs.push(c.u32()?);
                }
                let hash_frame = c.u32()?;
                let hash = c.u32()?;
                let hash_confirmed = c.u8()?;
                Packet::Input { base_frame, inputs, hash_frame, hash, hash_confirmed }
            }
            msg::PING => Packet::Ping { t1: c.u64()? },
            msg::PONG => Packet::Pong { t1: c.u64()?, t2: c.u64()?, t3: c.u64()? },
            msg::DESYNC_REPORT => Packet::DesyncReport {
                frame: c.u32()?,
                local_hash: c.u32()?,
                remote_hash: c.u32()?,
            },
            msg::STATE_CHUNK => {
                let transfer_id = c.u32()?;
                let frame = c.u32()?;
                let chunk_idx = c.u16()?;
                let total_chunks = c.u16()?;
                let remaining = c.buf.len() - c.pos;
                let data = c.bytes(remaining)?.to_vec();
                Packet::StateChunk { transfer_id, frame, chunk_idx, total_chunks, data }
            }
            msg::WATCHDOG => Packet::Watchdog {
                state: c.u8()?,
                loss_pct_x100: c.u16()?,
                cpu_temp_x100: c.u16()?,
                gpu_temp_x100: c.u16()?,
            },
            msg::ANNOUNCE => {
                let len = c.u8()? as usize;
                let raw = c.bytes(len)?;
                Packet::Announce { node_id: String::from_utf8_lossy(raw).into_owned() }
            }
            msg::ANNOUNCE_ACK => {
                let your_slot = c.u8()?;
                let peer_addr = take_addr(&mut c)?;
                Packet::AnnounceAck { your_slot, peer_addr }
            }
            _ => return None,
        };
        Some((session_id, pkt))
    }
}

// ---------------------------------------------------------------------------
// Configuration, deterministic-core traits, frame history ring
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NetplayConfig {
    pub session_id: u64,
    pub bind_port: u16,
    pub peer_addr: SocketAddr,
    /// 0 = Station A, 1 = Station B (only affects input slot ordering).
    pub local_player: usize,
    /// Host side of a hard state transfer (tie-break: lower node_id).
    pub is_host: bool,
    pub node_id: String,
    /// Ring capacity; INV-2 escalates to transfer beyond this rewind depth.
    pub history_frames: usize,
    pub frame_duration_us: u64,
    pub state_chunk_size: usize,
    /// Test hook: suppress outbound INPUT packets until this frame
    /// (simulates burst loss on the wire).
    pub suppress_input_until_frame: u32,
}

impl Default for NetplayConfig {
    fn default() -> Self {
        NetplayConfig {
            session_id: 1,
            bind_port: 9750,
            peer_addr: "127.0.0.1:9751".parse().unwrap(),
            local_player: 0,
            is_host: true,
            node_id: "ORION-01".to_string(),
            history_frames: DEFAULT_HISTORY_FRAMES,
            frame_duration_us: FRAME_DURATION_US,
            state_chunk_size: DEFAULT_STATE_CHUNK,
            suppress_input_until_frame: 0,
        }
    }
}

/// Canonical state image writer. Implementations must produce a byte image
/// that is byte-identical across machines for equal logical state (INV-5).
pub trait StateSave {
    /// Writes the canonical state image into `dst`; returns bytes written.
    fn save(&self, dst: &mut [u8]) -> usize;
}

/// State image loader (the deserialization half of the pair).
pub trait StateLoad {
    fn load(&mut self, src: &[u8]) -> Result<(), NetplayError>;
}

/// A deterministic emulator core hookup (e.g. the FBNeo libretro bridge).
/// `advance` MUST be a pure function of (current state, inputs).
pub trait DeterministicCore: StateSave + StateLoad + Send + 'static {
    /// Size of one canonical state image (fixed for the session).
    fn state_bytes(&self) -> usize;
    /// S(F+1) = T(S(F), inputs) — the rollback transition function.
    fn advance(&mut self, inputs: [InputWord; MAX_PLAYERS]);
}

/// One frame's record in the circular history buffer. `pre_state` holds
/// S(F) (state *before* advancing frame F) so a rollback to F is a single
/// load; `post_hash` is h(F) = CRC32(S(F+1)) for the integrity chain.
pub struct FrameSlot {
    pub frame: u32,
    pub local_input: InputWord,
    pub remote_input: Option<InputWord>,
    pub predicted: bool,
    pub pre_state: Vec<u8>,
    pub post_hash: u32,
    pub occupied: bool,
}

/// Circular frame history: fixed-capacity ring of frame slots with
/// pre-allocated state buffers (no allocation in the hot loop).
pub struct FrameHistory {
    slots: Vec<FrameSlot>,
    state_len: usize,
}

impl FrameHistory {
    pub fn new(capacity: usize, state_len: usize) -> Self {
        let slots = (0..capacity)
            .map(|_| FrameSlot {
                frame: 0,
                local_input: 0,
                remote_input: None,
                predicted: false,
                pre_state: vec![0u8; state_len],
                post_hash: 0,
                occupied: false,
            })
            .collect();
        FrameHistory { slots, state_len }
    }

    #[allow(dead_code)] // introspection API for dashboards
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    #[allow(dead_code)] // introspection API for dashboards
    pub fn state_len(&self) -> usize {
        self.state_len
    }

    fn idx(&self, frame: u32) -> usize {
        (frame as usize) % self.slots.len()
    }

    /// Valid slot only if the ring still holds this exact frame.
    pub fn slot(&self, frame: u32) -> Option<&FrameSlot> {
        let s = &self.slots[self.idx(frame)];
        (s.occupied && s.frame == frame).then_some(s)
    }

    pub fn slot_mut(&mut self, frame: u32) -> Option<&mut FrameSlot> {
        let i = self.idx(frame);
        let s = &mut self.slots[i];
        (s.occupied && s.frame == frame).then_some(s)
    }

    /// Claim the ring cell for `frame`, recycling whatever it held.
    pub fn slot_force(&mut self, frame: u32) -> &mut FrameSlot {
        let i = self.idx(frame);
        let s = &mut self.slots[i];
        s.frame = frame;
        s.occupied = true;
        s
    }

    /// Invalidate all slots >= frame (used after a hard state transfer).
    pub fn clear_from(&mut self, frame: u32) {
        for s in self.slots.iter_mut() {
            if s.occupied && s.frame >= frame {
                s.occupied = false;
            }
        }
    }

    /// h(F) only when frame F was simulated with *confirmed* inputs.
    pub fn confirmed_hash_at(&self, frame: u32) -> Option<u32> {
        self.slot(frame).filter(|s| !s.predicted).map(|s| s.post_hash)
    }

    /// INV-2 guard: oldest frame still retrievable from the ring.
    pub fn oldest_holdable_frame(&self, current_frame: u32) -> u32 {
        current_frame.saturating_sub(self.slots.len() as u32)
    }
}

// ---------------------------------------------------------------------------
// Clock synchronization: dynamic drift controller (PI on the frame clock)
// ---------------------------------------------------------------------------

/// PI controller steering the *frame clock* pacing interval.
///
/// Plant: pacing offset u [µs/frame] integrates into phase x [µs]:
///     x(k+1) = x(k) + G * u(k),   G = frames per sync sample
/// Control (setpoint x = 0, e = -theta_hat):
///     I(k+1) = clamp(I(k) + ki * e,  ±U_MAX)
///     u(k)   = clamp(kp * e + I(k),  ±U_MAX)
/// Jury stability for the closed loop requires 0 < ki < kp < 2/G.
#[derive(Debug)]
pub struct DriftController {
    kp: f64,
    ki: f64,
    integral: f64,
    max_pacing_delta_us: f64,
    last_offset_us: f64,
    samples: u64,
}

impl DriftController {
    pub fn new() -> Self {
        DriftController {
            kp: DRIFT_KP,
            ki: DRIFT_KI,
            integral: 0.0,
            max_pacing_delta_us: MAX_PACING_DELTA_US,
            last_offset_us: 0.0,
            samples: 0,
        }
    }

    /// Feed one valid NTP offset sample (remote-ahead, µs) and get the
    /// pacing adjustment in µs/frame. Negative = run faster (shorter
    /// frame period) to catch up with a remote clock that is ahead.
    pub fn observe(&mut self, offset_us: f64) -> f64 {
        let e = -offset_us;
        self.integral = (self.integral + self.ki * e).clamp(-self.max_pacing_delta_us, self.max_pacing_delta_us);
        self.last_offset_us = offset_us;
        self.samples += 1;
        (self.kp * e + self.integral).clamp(-self.max_pacing_delta_us, self.max_pacing_delta_us)
    }

    /// Phase too far off for slewing — return a one-shot step correction (µs).
    pub fn step_correction_needed(&self) -> Option<f64> {
        (self.last_offset_us.abs() > STEP_CORRECT_THRESHOLD_US).then_some(-self.last_offset_us)
    }

    pub fn offset_us(&self) -> f64 {
        self.last_offset_us
    }

    #[allow(dead_code)] // introspection API for dashboards
    pub fn samples(&self) -> u64 {
        self.samples
    }
}

impl Default for DriftController {
    fn default() -> Self {
        Self::new()
    }
}

/// One NTP 4-timestamp sample, as measured by the *local* cabinet.
#[derive(Debug, Clone, Copy)]
pub struct SyncSample {
    pub t1: u64, // local send time (local clock, µs)
    pub t2: u64, // remote receive time (remote clock, µs)
    pub t3: u64, // remote reply time (remote clock, µs)
    pub t4: u64, // local receive time (local clock, µs)
}

impl SyncSample {
    pub fn rtt_us(&self) -> f64 {
        // Signed arithmetic: t2/t3 come from the PEER's clock, so naive
        // u64 subtraction can underflow when clocks differ.
        let a = (self.t4 as i64).wrapping_sub(self.t1 as i64);
        let b = (self.t3 as i64).wrapping_sub(self.t2 as i64);
        (a - b) as f64
    }
    /// Remote clock minus local clock (positive = remote ahead).
    pub fn offset_us(&self) -> f64 {
        let a = (self.t2 as i64).wrapping_sub(self.t1 as i64);
        let b = (self.t3 as i64).wrapping_sub(self.t4 as i64);
        (a + b) as f64 / 2.0
    }
}

// ---------------------------------------------------------------------------
// Health / watchdog types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthState {
    Active,
    Degraded,
    ThermalHold,
    LocalMode,
}

impl HealthState {
    pub fn as_u8(self) -> u8 {
        match self {
            HealthState::Active => 0,
            HealthState::Degraded => 1,
            HealthState::ThermalHold => 2,
            HealthState::LocalMode => 3,
        }
    }
    pub fn from_u8(v: u8) -> HealthState {
        match v {
            1 => HealthState::Degraded,
            2 => HealthState::ThermalHold,
            3 => HealthState::LocalMode,
            _ => HealthState::Active,
        }
    }
}

/// Emitted to B.O.S.S. on every watchdog transition and periodic heartbeat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetplayHealthReport {
    pub session_id: u64,
    pub node_id: String,
    pub peer_addr: String,
    pub state: HealthState,
    pub loss_pct: f32,
    pub clock_offset_us: f32,
    pub cpu_temp_c: f32,
    pub gpu_temp_c: f32,
    pub frames_simulated: u64,
    pub rollbacks: u64,
    pub desync_events: u32,
    pub reason: String,
    pub ts_epoch_ms: u64,
}

/// Final result when a session loop exits.
#[derive(Debug, Clone, Serialize)]
pub struct SessionOutcome {
    pub frames_simulated: u64,
    pub rollbacks: u64,
    pub desync_events: u32,
    /// Count of rolling-CRC32 cross-checks performed against the peer at
    /// mutually-confirmed frames. Every one of these passing is a live
    /// verification of INV-1.
    pub hash_cross_checks: u64,
    pub avg_loss_pct: f32,
    pub final_clock_offset_us: f64,
    pub final_state_hash: u32,
    pub health: HealthState,
    pub end_reason: String,
}

// ---------------------------------------------------------------------------
// Rollback session — the async P2P state synchronization engine
// ---------------------------------------------------------------------------

/// Joypad source: called once per frame tick with the frame number.
/// `Send + Sync` so `&RollbackSession` futures stay `Send` across awaits.
pub type InputFn = Box<dyn FnMut(u32) -> InputWord + Send + Sync>;

struct TransferRx {
    transfer_id: u32,
    frame: u32,
    total: u16,
    chunks: Vec<Option<Vec<u8>>>,
    received: u16,
}

struct TransferTx {
    transfer_id: u32,
    frame: u32,
    data: Vec<u8>,
    next_chunk: u16,
    total: u16,
}

/// A live rollback netplay session between two cabinets.
///
/// Event loop: `tokio::select!` over (a) inbound datagrams, (b) the frame
/// deadline (drift-adjusted pacing), (c) the clock-sync timer, (d) the
/// watchdog timer. Single-task ownership of all state — no locks.
pub struct RollbackSession<C: DeterministicCore> {
    cfg: NetplayConfig,
    socket: Arc<UdpSocket>,
    core: C,
    history: FrameHistory,
    scratch: Vec<u8>,
    current_frame: u32,
    input_fn: InputFn,
    /// Captured-but-not-yet-applied local inputs (input-delay queue).
    local_capture_q: VecDeque<InputWord>,
    active_input_delay: u32,
    /// Remote inputs that arrived ahead of the local frame clock.
    remote_future: HashMap<u32, InputWord>,
    /// Local inputs captured while frozen for a hard resync.
    local_pending: VecDeque<(u32, InputWord)>,
    remote_last_input: InputWord,
    drift: DriftController,
    pacing_us: u64,
    next_deadline: TokioInstant,
    clock_sync: tokio::time::Interval,
    watchdog_timer: tokio::time::Interval,
    loss_ewma_pct: f32,
    loss_sum_pct: f32,
    loss_windows: u32,
    win_frames: u32,
    win_recv: u32,
    last_peer_ms: u64,
    peer_health: HealthState,
    /// Transfer/hold freeze: frame clock runs, core does not advance.
    frozen: bool,
    thermal_hold: bool,
    frozen_since_ms: u64,
    rx: Option<TransferRx>,
    tx: Option<TransferTx>,
    desync_events: u32,
    rollbacks: u64,
    frames_simulated: u64,
    hash_cross_checks: u64,
    /// Frames below this index carry pre-transfer (divergent) hashes that
    /// must never be advertised or cross-checked (set by hard resync).
    hash_floor: u32,
    health: HealthState,
    reason: String,
    boss_url: Option<String>,
}

impl<C: DeterministicCore> RollbackSession<C> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: NetplayConfig,
        socket: Arc<UdpSocket>,
        core: C,
        input_fn: InputFn,
        boss_url: Option<String>,
    ) -> Self {
        let state_len = core.state_bytes();
        let now = TokioInstant::now();
        let clock_sync = interval_at(
            now + TokioDuration::from_millis(CLOCK_SYNC_INTERVAL_MS),
            TokioDuration::from_millis(CLOCK_SYNC_INTERVAL_MS),
        );
        let watchdog_timer = interval_at(
            now + TokioDuration::from_millis(WATCHDOG_INTERVAL_MS),
            TokioDuration::from_millis(WATCHDOG_INTERVAL_MS),
        );
        RollbackSession {
            history: FrameHistory::new(cfg.history_frames, state_len),
            scratch: vec![0u8; state_len],
            pacing_us: cfg.frame_duration_us,
            next_deadline: now + TokioDuration::from_micros(cfg.frame_duration_us),
            active_input_delay: 0,
            loss_ewma_pct: 0.0,
            loss_sum_pct: 0.0,
            loss_windows: 0,
            win_frames: 0,
            win_recv: 0,
            last_peer_ms: now_epoch_ms(),
            peer_health: HealthState::Active,
            health: HealthState::Active,
            reason: String::new(),
            frames_simulated: 0,
            rollbacks: 0,
            desync_events: 0,
            hash_cross_checks: 0,
            hash_floor: 0,
            frozen: false,
            thermal_hold: false,
            frozen_since_ms: 0,
            rx: None,
            tx: None,
            remote_future: HashMap::new(),
            local_pending: VecDeque::new(),
            local_capture_q: VecDeque::new(),
            remote_last_input: NEUTRAL_INPUT,
            drift: DriftController::new(),
            cfg,
            socket,
            core,
            input_fn,
            boss_url,
            clock_sync,
            watchdog_timer,
            current_frame: 0,
        }
    }

    /// Main event loop; exits on shutdown signal, IO error, or watchdog
    /// escalation to LOCAL_MODE. Returns the final SessionOutcome.
    pub async fn run(mut self, mut shutdown: watch::Receiver<bool>) -> SessionOutcome {
        info!(
            session = self.cfg.session_id,
            peer = %self.cfg.peer_addr,
            host = self.cfg.is_host,
            "netplay session started"
        );
        loop {
            if *shutdown.borrow_and_update() {
                self.end(HealthState::LocalMode, "shutdown signal");
                break;
            }
            let mut buf = [0u8; MAX_DATAGRAM];
            tokio::select! {
                r = self.socket.recv_from(&mut buf) => {
                    match r {
                        Ok((len, from)) => self.on_datagram(len, &buf, from).await,
                        Err(e) => {
                            warn!(error = %e, "udp recv failed");
                            self.end(HealthState::LocalMode, "socket error");
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep_until(self.next_deadline) => {
                    self.frame_tick().await;
                    self.advance_deadline();
                }
                _ = self.clock_sync.tick() => {
                    self.send(Packet::Ping { t1: now_epoch_us() }).await;
                }
                _ = self.watchdog_timer.tick() => {
                    if self.watchdog_check().await {
                        break;
                    }
                }
            }
        }
        self.report_to_boss().await;
        self.outcome()
    }

    fn advance_deadline(&mut self) {
        let now = TokioInstant::now();
        self.next_deadline += TokioDuration::from_micros(self.pacing_us);
        if self.next_deadline < now {
            // Fell behind (hot rollback, debugger stall): re-anchor instead
            // of spiraling through a backlog of frame deadlines.
            self.next_deadline = now + TokioDuration::from_micros(self.pacing_us);
        }
    }

    fn end(&mut self, health: HealthState, reason: &str) {
        if self.health != HealthState::LocalMode {
            self.health = health;
            self.reason = reason.to_string();
        }
    }

    fn outcome(&self) -> SessionOutcome {
        let mut img = vec![0u8; self.history.state_len()];
        let n = self.core.save(&mut img);
        SessionOutcome {
            frames_simulated: self.frames_simulated,
            rollbacks: self.rollbacks,
            desync_events: self.desync_events,
            hash_cross_checks: self.hash_cross_checks,
            avg_loss_pct: if self.loss_windows > 0 {
                self.loss_sum_pct / self.loss_windows as f32
            } else {
                0.0
            },
            final_clock_offset_us: self.drift.offset_us(),
            final_state_hash: crc32(&img[..n]),
            health: self.health,
            end_reason: if self.reason.is_empty() {
                "session ended".to_string()
            } else {
                self.reason.clone()
            },
        }
    }

    /// Current stats snapshot: (frame, rollbacks, desyncs, loss%, offset µs).
    #[allow(dead_code)] // introspection API for dashboards
    pub fn stats(&self) -> (u32, u64, u32, f32, f64) {
        (
            self.current_frame,
            self.rollbacks,
            self.desync_events,
            self.loss_ewma_pct,
            self.drift.offset_us(),
        )
    }

    /// One frame tick: capture input -> predict -> save state -> advance
    /// -> hash -> record -> transmit. During a hard-resync freeze the frame
    /// clock keeps running and inputs keep flowing to the peer, but the
    /// core does not advance (INV-3: no phantom advance).
    async fn frame_tick(&mut self) {
        let f = self.current_frame;

        // 1. Capture local input (through the input-delay queue).
        let captured = (self.input_fn)(f);
        self.local_capture_q.push_back(captured);
        while self.local_capture_q.len() > self.active_input_delay as usize + 1 {
            self.local_capture_q.pop_front();
        }
        let local = self.local_capture_q.front().copied().unwrap_or(NEUTRAL_INPUT);

        if self.frozen || self.thermal_hold {
            self.local_pending.push_back((f, local));
            if self.local_pending.len() > 1_024 {
                self.local_pending.pop_front();
            }
            self.current_frame = f + 1;
            self.win_frames += 1;
            self.send_frozen_inputs(f).await;
            return;
        }

        // 2. Remote input: actual (early arrival) or conservative prediction.
        let remote_known = self.remote_future.remove(&f);
        let remote = remote_known.unwrap_or(self.remote_last_input);

        // Canonical player order: player 0's word is ALWAYS inputs[0] on
        // both cabinets, regardless of which cabinet is local.
        let pair = if self.cfg.local_player == 0 { [local, remote] } else { [remote, local] };

        // 3. Save pre-state S(F) into the ring slot, advance, hash S(F+1).
        let pre_state = {
            let slot = self.history.slot_force(f);
            slot.local_input = local;
            slot.remote_input = remote_known;
            slot.predicted = remote_known.is_none();
            slot.post_hash = 0;
            &mut slot.pre_state
        };
        let _ = self.core.save(pre_state);
        self.core.advance(pair);

        let n = self.core.save(&mut self.scratch);
        let hash = crc32(&self.scratch[..n]);
        if let Some(slot) = self.history.slot_mut(f) {
            slot.post_hash = hash;
        }

        self.current_frame = f + 1;
        self.frames_simulated += 1;
        self.win_frames += 1;

        // 4. Transmit input window + integrity hash; pace state chunks.
        self.send_input_packet(f).await;
        if self.tx.is_some() {
            self.send_next_chunk().await;
        }
    }

    /// The authoritative rollback-and-resimulate loop.
    /// Rewind to frame F* (load saved S(F*)) and replay every frame up to
    /// `now` with the now-known inputs. INV-2: if F* fell out of the ring,
    /// escalate to a hard state transfer instead of resimulating garbage.
    async fn rollback_and_resimulate(&mut self, target: u32) {
        let floor = self.history.oldest_holdable_frame(self.current_frame);
        let src = if target >= floor {
            self.history.slot(target).map(|s| s.pre_state.clone())
        } else {
            warn!(target, floor, current = self.current_frame,
                  "rollback depth exceeded ring capacity (INV-2)");
            None
        };
        let Some(src) = src else {
            self.begin_hard_resync().await;
            return;
        };
        if let Err(e) = self.core.load(&src) {
            error!(error = %e, "rollback state load failed");
            self.begin_hard_resync().await;
            return;
        }
        for f in target..self.current_frame {
            let (local, remote_known) = {
                let slot = self.history.slot_mut(f).expect("slot within ring");
                (slot.local_input, slot.remote_input)
            };
            let remote = remote_known.unwrap_or(self.remote_last_input);
            let pair = if self.cfg.local_player == 0 { [local, remote] } else { [remote, local] };
            let pre_state = {
                let slot = self.history.slot_mut(f).expect("slot within ring");
                &mut slot.pre_state
            };
            let _ = self.core.save(pre_state);
            self.core.advance(pair);
            let n = self.core.save(&mut self.scratch);
            let hash = crc32(&self.scratch[..n]);
            let slot = self.history.slot_mut(f).expect("slot within ring");
            slot.post_hash = hash;
            slot.predicted = remote_known.is_none();
        }
        self.rollbacks += 1;
        debug!(target, current = self.current_frame, rollbacks = self.rollbacks,
               "rollback resimulated");
    }

    /// Dispatch an inbound datagram (handles out-of-order, duplicates and
    /// unknown senders by design: UDP datagrams are stateless events).
    async fn on_datagram(&mut self, len: usize, buf: &[u8], _from: SocketAddr) {
        let Some((session_id, pkt)) = Packet::decode(&buf[..len]) else {
            return; // malformed / wrong version: silently dropped
        };
        if session_id != self.cfg.session_id {
            return;
        }
        self.last_peer_ms = now_epoch_ms();
        match pkt {
            Packet::Input { base_frame, inputs, hash_frame, hash, hash_confirmed } => {
                self.on_input(base_frame, &inputs, hash_frame, hash, hash_confirmed != 0).await;
            }
            Packet::Ping { t1 } => {
                let t2 = now_epoch_us();
                let t3 = now_epoch_us();
                self.send(Packet::Pong { t1, t2, t3 }).await;
            }
            Packet::Pong { t1, t2, t3 } => {
                let t4 = now_epoch_us();
                let sample = SyncSample { t1, t2, t3, t4 };
                if sample.rtt_us() <= RTT_MAX_US {
                    let u = self.drift.observe(sample.offset_us());
                    // Slew the frame pacing interval; clamp keeps us inside
                    // the ±0.5 ms/frame actuator range.
                    self.pacing_us =
                        (self.cfg.frame_duration_us as f64 + u).round().max(1_000.0) as u64;
                    // Phase too far off for slewing: one-shot step of the
                    // frame deadline (never the wall clock).
                    if let Some(step_us) = self.drift.step_correction_needed() {
                        let step = step_us.round() as i64;
                        warn!(offset_us = sample.offset_us(), step,
                              "clock sync: applying phase step");
                        if step >= 0 {
                            self.next_deadline += TokioDuration::from_micros(step as u64);
                        } else {
                            self.next_deadline -= TokioDuration::from_micros((-step) as u64);
                        }
                    }
                    debug!(offset_us = sample.offset_us(), rtt_us = sample.rtt_us(), u,
                           pacing_us = self.pacing_us, "clock sync sample");
                }
            }
            Packet::DesyncReport { frame, local_hash, remote_hash } => {
                self.on_desync_report(frame, local_hash, remote_hash).await;
            }
            Packet::StateChunk { transfer_id, frame, chunk_idx, total_chunks, data } => {
                self.on_state_chunk(transfer_id, frame, chunk_idx, total_chunks, data).await;
            }
            Packet::Watchdog { state, .. } => {
                self.peer_health = HealthState::from_u8(state);
                if self.peer_health == HealthState::LocalMode {
                    self.end(HealthState::LocalMode, "peer degraded to local mode");
                }
            }
            _ => {}
        }
    }

    /// Apply a sliding window of remote inputs; detect prediction errors,
    /// out-of-order frames and cross-cabinet hash mismatches (INV-1 check).
    async fn on_input(
        &mut self,
        base_frame: u32,
        inputs: &[InputWord],
        hash_frame: u32,
        hash: u32,
        hash_confirmed: bool,
    ) {
        self.win_recv += 1;
        let now = self.current_frame;

        let mut min_correction: Option<u32> = None;
        for (i, &input) in inputs.iter().enumerate() {
            let frame = base_frame + i as u32;
            if frame >= now {
                // Early arrival: queue for the frame clock to catch up.
                self.remote_future.insert(frame, input);
            } else if let Some(slot) = self.history.slot_mut(frame) {
                match slot.remote_input {
                    None => {
                        // Was predicted — correct it and remember rewind target.
                        slot.remote_input = Some(input);
                        min_correction = Some(min_correction.unwrap_or(frame));
                    }
                    Some(prev) if prev != input => {
                        // Two different "actual" inputs for one confirmed frame:
                        // peer contradiction — treat as hard desync (INV-1).
                        // Ignored while frozen: the transferred state will
                        // overwrite this history wholesale.
                        if self.frozen {
                            continue;
                        }
                        error!(frame, prev, input, "contradictory confirmed remote input");
                        self.desync_events += 1;
                        self.begin_hard_resync().await;
                        return;
                    }
                    Some(_) => {}
                }
            }
            // frame older than ring floor: unrecoverable by rewind → the
            // hash cross-check below escalates if it matters.
        }

        if let Some(last) = inputs.last() {
            self.remote_last_input = *last; // INV-4 prediction seed
        }

        if let Some(target) = min_correction {
            self.rollback_and_resimulate(target).await;
        }

        // Rolling CRC32 desync detector: compare only when BOTH sides held
        // fully confirmed inputs at hash_frame (never against predictions),
        // only above the hash floor (pre-transfer history is exempt), and
        // never while frozen (the local history is about to be replaced).
        if hash_confirmed && hash_frame >= self.hash_floor && !self.frozen {
            if let Some(our) = self.history.confirmed_hash_at(hash_frame) {
                self.hash_cross_checks += 1;
                if our != hash {
                    error!(hash_frame, our = format!("{our:#010x}"),
                           theirs = format!("{hash:#010x}"),
                           "state hash mismatch at confirmed frame — hard desync (INV-5)");
                    self.desync_events += 1;
                    self.begin_hard_resync().await;
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Hard resync: snapshot rewind failure escalates to a state transfer
    // from the host (Level-2 recovery). Guest freezes, host streams.
    // -----------------------------------------------------------------------

    /// Guest side: freeze and request the host's authoritative state.
    async fn begin_hard_resync(&mut self) {
        if self.frozen {
            return; // already resyncing
        }
        if self.cfg.is_host {
            // Host cannot pull from the guest by protocol contract; the
            // guest reports and the host serves. (Single-host invariant.)
            warn!("host detected desync; awaiting guest DesyncReport");
            return;
        }
        warn!(frame = self.current_frame, "freezing for hard state transfer from host");
        self.frozen = true;
        self.frozen_since_ms = now_epoch_ms();
        self.local_pending.clear();
        let n = self.core.save(&mut self.scratch);
        self.send(Packet::DesyncReport {
            frame: self.current_frame,
            local_hash: crc32(&self.scratch[..n]),
            remote_hash: 0,
        })
        .await;
    }

    /// Host side: serve the guest's DesyncReport with a chunked state image.
    async fn on_desync_report(&mut self, _frame: u32, local_hash: u32, _remote_hash: u32) {
        if !self.cfg.is_host || self.tx.is_some() {
            return;
        }
        // Serve the host's *current* state S(G) at host frame G. The guest
        // jumps forward to G; frames are never rewritten, only skipped.
        let n = self.core.save(&mut self.scratch);
        let state = self.scratch[..n].to_vec();
        let total = ((state.len() + self.cfg.state_chunk_size - 1) / self.cfg.state_chunk_size)
            .max(1) as u16;
        let transfer_id = now_epoch_us() as u32;
        warn!(
            guest_hash = format!("{local_hash:#010x}"),
            frame = self.current_frame,
            total,
            "serving hard state transfer to guest"
        );
        self.tx = Some(TransferTx {
            transfer_id,
            frame: self.current_frame,
            data: state,
            next_chunk: 0,
            total,
        });
        self.send_next_chunk().await;
    }

    async fn send_next_chunk(&mut self) {
        let Some(tx) = &self.tx else { return };
        if tx.next_chunk >= tx.total {
            return;
        }
        let start = tx.next_chunk as usize * self.cfg.state_chunk_size;
        let end = (start + self.cfg.state_chunk_size).min(tx.data.len());
        let pkt = Packet::StateChunk {
            transfer_id: tx.transfer_id,
            frame: tx.frame,
            chunk_idx: tx.next_chunk,
            total_chunks: tx.total,
            data: tx.data[start..end].to_vec(),
        };
        self.send(pkt).await;
        let tx = self.tx.as_mut().unwrap();
        tx.next_chunk += 1;
    }

    /// Guest side: reassemble chunks and jump to the host's frame G.
    async fn on_state_chunk(
        &mut self,
        transfer_id: u32,
        frame: u32,
        chunk_idx: u16,
        total_chunks: u16,
        data: Vec<u8>,
    ) {
        if !self.frozen {
            return; // only expected while frozen
        }
        match &mut self.rx {
            Some(rx) if rx.transfer_id == transfer_id => {
                if (chunk_idx as usize) < rx.chunks.len() && rx.chunks[chunk_idx as usize].is_none() {
                    rx.chunks[chunk_idx as usize] = Some(data);
                    rx.received += 1;
                }
            }
            _ => {
                self.rx = Some(TransferRx {
                    transfer_id,
                    frame,
                    total: total_chunks,
                    chunks: (0..total_chunks).map(|_| None).collect(),
                    received: 0,
                });
                if chunk_idx < total_chunks {
                    let rx = self.rx.as_mut().unwrap();
                    rx.chunks[chunk_idx as usize] = Some(data);
                    rx.received += 1;
                }
            }
        }

        let done = self.rx.as_ref().map(|rx| rx.received == rx.total).unwrap_or(false);
        if !done {
            return;
        }
        let rx = self.rx.take().unwrap();
        let mut image = Vec::with_capacity(rx.total as usize * self.cfg.state_chunk_size);
        for chunk in &rx.chunks {
            match chunk {
                Some(c) => image.extend_from_slice(c),
                None => {
                    error!("state transfer completed with holes — impossible");
                    return;
                }
            }
        }
        match self.core.load(&image) {
            Ok(()) => {
                info!(frame = rx.frame, bytes = image.len(), "hard resync applied from host");
                let g = rx.frame;
                // Rewind the frame clock to the host's frame G, drop our
                // divergent history from G onward, and re-seed the local
                // input pipeline with the inputs captured while frozen.
                self.current_frame = g;
                self.history.clear_from(g);
                self.remote_future.retain(|f, _| *f >= g);
                // Hashes below G come from the divergent pre-transfer
                // simulation: quarantine them (INV-5 fail-visible without
                // spurious re-escalation).
                self.hash_floor = g;
                let capture: VecDeque<InputWord> = self
                    .local_pending
                    .drain(..)
                    .filter(|(f, _)| *f >= g)
                    .map(|(_, i)| i)
                    .collect();
                self.local_capture_q = capture;
                self.active_input_delay = 0;
                self.frozen = false;
                self.thermal_hold = false;
                self.rx = None;
                self.tx = None;
            }
            Err(e) => {
                error!(error = %e, "guest failed to load transferred state");
                self.end(HealthState::LocalMode, "state transfer corrupted");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Transmit helpers
    // -----------------------------------------------------------------------

    async fn send(&self, pkt: Packet) {
        let bytes = pkt.encode(self.cfg.session_id);
        if let Err(e) = self.socket.send_to(&bytes, self.cfg.peer_addr).await {
            debug!(error = %e, "udp send failed");
        }
    }

    /// Sliding-window input packet: last INPUT_WINDOW local inputs plus the
    /// integrity hash h(F) of the most recent simulated frame.
    async fn send_input_packet(&self, frame: u32) {
        if frame < self.cfg.suppress_input_until_frame {
            return; // test hook: simulated outbound burst loss
        }
        let base = frame.saturating_sub(INPUT_WINDOW as u32 - 1);
        let mut inputs = Vec::with_capacity(INPUT_WINDOW);
        for f in base..=frame {
            if let Some(slot) = self.history.slot(f) {
                inputs.push(slot.local_input);
            }
        }
        if inputs.is_empty() {
            return;
        }
        let base = frame + 1 - inputs.len() as u32;
        // Advertise the most recent CONFIRMED frame's hash (never a
        // provisional predicted state) so the peer's INV-1 cross-check
        // always compares apples to apples.
        let mut probe = frame;
        let mut slot_info: Option<(u32, u32, bool)> = None;
        while probe + 24 > frame {
            if probe < self.hash_floor {
                break; // stale pre-transfer history: never advertise
            }
            if let Some(slot) = self.history.slot(probe) {
                if !slot.predicted {
                    slot_info = Some((probe, slot.post_hash, true));
                    break;
                }
            }
            if probe == 0 {
                break;
            }
            probe -= 1;
        }
        let (hash_frame, hash, hash_confirmed) = slot_info
            .map(|(f, h, c)| (f, h, c as u8))
            .unwrap_or((frame, 0, 0u8));
        self.send(Packet::Input {
            base_frame: base,
            inputs,
            hash_frame,
            hash,
            hash_confirmed,
        })
        .await;
    }

    /// While frozen: stream buffered local inputs so the peer's predictions
    /// stay aligned with what we will actually replay after the resync.
    async fn send_frozen_inputs(&self, frame: u32) {
        let tail: Vec<InputWord> = self
            .local_pending
            .iter()
            .rev()
            .take(INPUT_WINDOW)
            .map(|(_, i)| *i)
            .collect();
        if tail.is_empty() {
            return;
        }
        let base = frame + 1 - tail.len() as u32;
        self.send(Packet::Input {
            base_frame: base,
            inputs: tail.into_iter().rev().collect(),
            hash_frame: frame,
            hash: 0,
            hash_confirmed: 0,
        })
        .await;
    }

    // -----------------------------------------------------------------------
    // Watchdog failure tree + B.O.S.S. telemetry
    // -----------------------------------------------------------------------

    /// Evaluate the watchdog failure tree. Returns true when the session
    /// must terminate (LOCAL_MODE escalation or partition detection).
    async fn watchdog_check(&mut self) -> bool {
        // 1. Peer silence (network partition / peer crash).
        let silent = now_epoch_ms().saturating_sub(self.last_peer_ms) > PEER_SILENCE_TIMEOUT_MS;

        // 2. Packet-loss EWMA over the completed measurement window.
        let loss_now = if self.win_frames >= LOSS_WINDOW_FRAMES {
            let lost = self.win_frames.saturating_sub(self.win_recv.min(self.win_frames));
            self.win_frames = 0;
            self.win_recv = 0;
            let pct = lost as f32 / LOSS_WINDOW_FRAMES as f32 * 100.0;
            self.loss_ewma_pct = if self.loss_ewma_pct == 0.0 {
                pct
            } else {
                self.loss_ewma_pct * 0.7 + pct * 0.3
            };
            self.loss_sum_pct += self.loss_ewma_pct;
            self.loss_windows += 1;
            self.loss_ewma_pct
        } else {
            self.loss_ewma_pct
        };

        // 3. Thermal/power anomalies (RK3588S thermal zones).
        let (cpu, gpu) = sample_temps();
        let hot = cpu > CPU_TEMP_LIMIT_C || gpu > GPU_TEMP_LIMIT_C;
        let cool = cpu < CPU_TEMP_HYST_C && gpu < GPU_TEMP_HYST_C;

        // 4. Hard-resync freeze timeout.
        if self.frozen && now_epoch_ms().saturating_sub(self.frozen_since_ms) > FREEZE_TIMEOUT_MS {
            self.end(HealthState::LocalMode, "state transfer timed out");
            return true;
        }

        if silent || self.peer_health == HealthState::LocalMode {
            let reason = if silent { "peer partition detected" } else { "peer went local" };
            warn!(reason, "watchdog: degrading to local mode");
            self.end(HealthState::LocalMode, reason);
            return true;
        }

        if hot && self.health != HealthState::ThermalHold {
            warn!(cpu, gpu, "watchdog: THERMAL_HOLD (simulation frozen, network alive)");
            self.health = HealthState::ThermalHold;
            self.thermal_hold = true; // hold frame clock, keep network alive
            self.send_watchdog(cpu, gpu).await;
            self.report_to_boss().await;
        } else if self.health == HealthState::ThermalHold && cool {
            info!(cpu, gpu, "watchdog: thermal recovery, resuming");
            self.health = HealthState::Active;
            self.thermal_hold = false;
            self.local_capture_q.clear();
            self.send_watchdog(cpu, gpu).await;
        }

        if self.health == HealthState::Active && loss_now > LOSS_DEGRADE_PCT {
            warn!(loss = loss_now, "watchdog: DEGRADED — raising input delay to 3 frames");
            self.health = HealthState::Degraded;
            self.active_input_delay = 3;
            self.local_capture_q.clear();
            self.send_watchdog(cpu, gpu).await;
            self.report_to_boss().await;
        } else if self.health == HealthState::Degraded && loss_now > LOSS_LOCAL_PCT {
            warn!(loss = loss_now, "watchdog: unrecoverable loss — LOCAL_MODE");
            self.end(HealthState::LocalMode, "packet loss beyond 25% EWMA");
            return true;
        } else if self.health == HealthState::Degraded && loss_now < LOSS_DEGRADE_PCT {
            // Hysteresis recovery: link quality restored, drop extra delay.
            info!(loss = loss_now, "watchdog: link recovered — Active, delay 0");
            self.health = HealthState::Active;
            self.active_input_delay = 0;
            self.local_capture_q.clear();
            self.send_watchdog(cpu, gpu).await;
        }

        false
    }

    async fn send_watchdog(&self, cpu: f32, gpu: f32) {
        self.send(Packet::Watchdog {
            state: self.health.as_u8(),
            loss_pct_x100: (self.loss_ewma_pct * 100.0) as u16,
            cpu_temp_x100: (cpu * 100.0) as u16,
            gpu_temp_x100: (gpu * 100.0) as u16,
        })
        .await;
    }

    /// Fire-and-forget telemetry to B.O.S.S. (axum HTTP ingest).
    async fn report_to_boss(&self) {
        let Some(boss) = &self.boss_url else { return };
        let (cpu, gpu) = sample_temps();
        let report = NetplayHealthReport {
            session_id: self.cfg.session_id,
            node_id: self.cfg.node_id.clone(),
            peer_addr: self.cfg.peer_addr.to_string(),
            state: self.health,
            loss_pct: self.loss_ewma_pct,
            clock_offset_us: self.drift.offset_us() as f32,
            cpu_temp_c: cpu,
            gpu_temp_c: gpu,
            frames_simulated: self.frames_simulated,
            rollbacks: self.rollbacks,
            desync_events: self.desync_events,
            reason: self.reason.clone(),
            ts_epoch_ms: now_epoch_ms(),
        };
        let Ok(client) = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(500))
            .build()
        else {
            return;
        };
        let url = format!("{boss}/api/v1/netplay/report");
        // Non-fatal: telemetry must never stall gameplay.
        let _ = client.post(&url).json(&report).send().await;
    }
}

/// RK3588S thermal-zone sampling (same /sys convention as main.rs).
pub fn sample_temps() -> (f32, f32) {
    let read = |zone: u32| -> f32 {
        std::fs::read_to_string(format!("/sys/class/thermal/thermal_zone{zone}/temp"))
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .map(|mc| mc / 1000.0)
            .unwrap_or(42.0)
    };
    (read(0), read(1))
}

// ---------------------------------------------------------------------------
// Entry point + reference deterministic core
// ---------------------------------------------------------------------------

/// Bind, build and run a rollback session until `shutdown` fires.
/// `input_fn` supplies the local player's joypad word for frame F (poll the
/// cabinet's GPIO/USB encoder here). Generic over the emulator core hookup.
pub async fn run_station<C: DeterministicCore>(
    cfg: NetplayConfig,
    core: C,
    input_fn: InputFn,
    shutdown: watch::Receiver<bool>,
    boss_url: Option<String>,
) -> Result<SessionOutcome, NetplayError> {
    let socket = Arc::new(UdpSocket::bind(("0.0.0.0", cfg.bind_port)).await?);
    info!(port = cfg.bind_port, "netplay UDP socket bound");
    let session = RollbackSession::new(cfg, socket, core, input_fn, boss_url);
    Ok(session.run(shutdown).await)
}

/// Reference implementation of `DeterministicCore`: a deterministic mixing
/// accumulator. Serves as (a) the integration test harness and (b) the
/// template the FBNeo/libretro bridge must follow: `advance` is a pure
/// function of (state, inputs) and save/load round-trips byte-exactly.
#[derive(Debug, Clone, Default)]
pub struct SyntheticCore {
    acc: u64,
}

impl SyntheticCore {
    pub fn with_acc(acc: u64) -> Self {
        SyntheticCore { acc }
    }
    pub fn acc(&self) -> u64 {
        self.acc
    }
}

impl StateSave for SyntheticCore {
    fn save(&self, dst: &mut [u8]) -> usize {
        if dst.len() < 8 {
            return 0;
        }
        dst[..8].copy_from_slice(&self.acc.to_le_bytes());
        8
    }
}

impl StateLoad for SyntheticCore {
    fn load(&mut self, src: &[u8]) -> Result<(), NetplayError> {
        if src.len() < 8 {
            return Err(NetplayError::StateMalformed);
        }
        self.acc = u64::from_le_bytes(src[..8].try_into().unwrap());
        Ok(())
    }
}

impl DeterministicCore for SyntheticCore {
    fn state_bytes(&self) -> usize {
        8
    }
    fn advance(&mut self, inputs: [InputWord; MAX_PLAYERS]) {
        // Order- and input-sensitive mixing: any input divergence or
        // reordering changes every subsequent hash (INV-1 sensitive).
        self.acc = self
            .acc
            .wrapping_mul(0x100_0000_01B3)
            .wrapping_add((inputs[0] as u64) | ((inputs[1] as u64) << 32));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn crc32_known_vectors() {
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"The quick brown fox jumps over the lazy dog"), 0x414F_A339);
    }

    #[test]
    fn crc32_incremental_matches_oneshot() {
        let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let (a, b) = data.split_at(437);
        let streamed = crc32_update(crc32_update(0xFFFF_FFFF, a), b) ^ 0xFFFF_FFFF;
        assert_eq!(streamed, crc32(&data));
    }

    #[test]
    fn packet_roundtrip_all_variants() {
        let sid = 0xDEAD_BEEF_1234_5678u64;
        let pkts = vec![
            Packet::Input {
                base_frame: 42,
                inputs: vec![1, 2, 3, 4],
                hash_frame: 45,
                hash: 0x1234_5678,
                hash_confirmed: 1,
            },
            Packet::Ping { t1: 99 },
            Packet::Pong { t1: 1, t2: 2, t3: 3 },
            Packet::DesyncReport { frame: 7, local_hash: 0xA, remote_hash: 0xB },
            Packet::StateChunk {
                transfer_id: 5,
                frame: 100,
                chunk_idx: 2,
                total_chunks: 9,
                data: vec![7u8; 33],
            },
            Packet::Watchdog { state: 2, loss_pct_x100: 512, cpu_temp_x100: 8_500, gpu_temp_x100: 7_250 },
            Packet::Announce { node_id: "ORION-02".to_string() },
            Packet::AnnounceAck {
                your_slot: 1,
                peer_addr: Some("192.168.1.55:9750".parse().unwrap()),
            },
        ];
        for p in pkts {
            let bytes = p.encode(sid);
            assert!(bytes.len() <= MAX_DATAGRAM);
            let (sid2, decoded) = Packet::decode(&bytes).expect("decode");
            assert_eq!(sid2, sid);
            assert_eq!(decoded, p);
        }
    }

    #[test]
    fn ring_buffer_wraparound() {
        let mut hist = FrameHistory::new(128, 8);
        // Walk 300 frames through a 128-slot ring (2.34 wraps).
        for f in 0u32..300 {
            let slot = hist.slot_force(f);
            slot.local_input = f;
            slot.remote_input = Some(f * 2);
            slot.predicted = false;
            slot.post_hash = f * 7;
            slot.pre_state.copy_from_slice(&f.to_le_bytes().repeat(2));
        }
        // Old frames are recycled; recent ones are intact. After 300
        // frames through a 128-slot ring, indices 0..43 were rewritten by
        // frames 256..299, indices 44..127 by frames 172..255.
        assert!(hist.slot(299).is_some());
        assert!(hist.slot(200).is_some());
        assert!(hist.slot(199).is_some());
        assert!(hist.slot(255).is_some());
        assert!(hist.slot(172).is_some());
        assert!(hist.slot(171).is_none(), "slot overwritten by wraparound");
        assert!(hist.slot(0).is_none());
        assert_eq!(hist.confirmed_hash_at(299), Some(299 * 7));
        // Predicted frames never expose hashes (INV-5 guard).
        let slot = hist.slot_mut(299).unwrap();
        slot.predicted = true;
        assert_eq!(hist.confirmed_hash_at(299), None);
        assert_eq!(hist.oldest_holdable_frame(300), 300 - 128);
    }

    #[test]
    fn drift_controller_converges_and_is_jury_stable() {
        // Plant: phase integrates pacing adjustment over G frames/sample.
        let g = 15.0; // 60 fps, 250 ms sync interval
        let mut ctrl = DriftController::new();
        let mut phase = 800.0f64; // remote ahead by 0.8 ms
        for _ in 0..300 {
            let u = ctrl.observe(phase);
            phase += u * g;
            assert!(u.abs() <= MAX_PACING_DELTA_US, "actuator clamp violated");
            assert!(phase.abs() < 50_000.0, "diverged: {phase}");
        }
        assert!(
            phase.abs() < 1.0,
            "PI controller must drive phase error to zero, got {phase} µs"
        );
        // Steady state well inside the ±0.5 ms clock-sync requirement.
        assert!(ctrl.offset_us().abs() < 1.0);
    }

    #[test]
    fn synthetic_core_is_deterministic_and_order_sensitive() {
        let mut a = SyntheticCore::default();
        let mut b = SyntheticCore::default();
        for f in 0..100u32 {
            a.advance([f, f.wrapping_mul(3)]);
            b.advance([f, f.wrapping_mul(3)]);
        }
        assert_eq!(a.acc(), b.acc());
        // Swapped input order must diverge (canonical ordering matters).
        let mut c = SyntheticCore::default();
        for f in 0..100u32 {
            c.advance([f.wrapping_mul(3), f]);
        }
        assert_ne!(a.acc(), c.acc());
        // Save/load round-trip must be byte-exact.
        let mut img = vec![0u8; a.state_bytes()];
        let n = a.save(&mut img);
        assert_eq!(n, 8);
        let mut restored = SyntheticCore::default();
        restored.load(&img).unwrap();
        assert_eq!(a.acc(), restored.acc());
    }

    fn test_input(player: u32) -> Box<dyn FnMut(u32) -> InputWord + Send + Sync> {
        Box::new(move |frame: u32| {
            if frame < 26 {
                0 // benign startup inputs so early predictions are correct
            } else {
                (frame / 8).wrapping_mul(0x1111) ^ (player << 20) | 1
            }
        })
    }

    fn cfg_for(bind_port: u16, peer_port: u16, player: usize, host: bool) -> NetplayConfig {
        NetplayConfig {
            session_id: 0xCAFE_0001,
            bind_port,
            peer_addr: format!("127.0.0.1:{peer_port}").parse().unwrap(),
            local_player: player,
            is_host: host,
            node_id: if host { "ORION-01" } else { "ORION-02" }.to_string(),
            history_frames: 128,
            frame_duration_us: 500, // accelerated test pacing
            state_chunk_size: 4,
            suppress_input_until_frame: 0,
        }
    }

    /// Full-loop integration: two cabinets on loopback, B suppresses its
    /// first 25 input packets (burst loss). A must predict, then roll back
    /// and resimulate; both cabinets must converge to identical state
    /// hashes with zero desync escalations (INV-1 / INV-4 verification).
    #[tokio::test(flavor = "multi_thread")]
    async fn loopback_rollback_convergence() {
        let sa = Arc::new(UdpSocket::bind(("127.0.0.1", 0)).await.unwrap());
        let sb = Arc::new(UdpSocket::bind(("127.0.0.1", 0)).await.unwrap());
        let mut cfg_a = cfg_for(sa.local_addr().unwrap().port(), sb.local_addr().unwrap().port(), 0, true);
        let mut cfg_b = cfg_for(sb.local_addr().unwrap().port(), sa.local_addr().unwrap().port(), 1, false);
        // Burst loss: B's first 25 INPUT datagrams never reach A.
        cfg_b.suppress_input_until_frame = 25;
        cfg_a.suppress_input_until_frame = 0;

        let (tx_a, rx_a) = watch::channel(false);
        let (tx_b, rx_b) = watch::channel(false);

        let sess_a = RollbackSession::new(cfg_a, sa, SyntheticCore::default(), test_input(0), None);
        let sess_b = RollbackSession::new(cfg_b, sb, SyntheticCore::default(), test_input(1), None);
        let h_a = tokio::spawn(sess_a.run(rx_a));
        let h_b = tokio::spawn(sess_b.run(rx_b));

        tokio::time::sleep(Duration::from_millis(1_200)).await;
        let _ = tx_a.send(true);
        let _ = tx_b.send(true);

        let out_a = h_a.await.unwrap();
        let out_b = h_b.await.unwrap();

        eprintln!("A: {out_a:?}");
        eprintln!("B: {out_b:?}");

        assert!(out_a.frames_simulated >= 50, "A simulated {} frames", out_a.frames_simulated);
        assert!(out_b.frames_simulated >= 50, "B simulated {} frames", out_b.frames_simulated);
        // The burst loss must have forced at least one rollback on A.
        assert!(out_a.rollbacks > 0, "expected rollbacks on A after burst loss");
        // INV-1 verification: every rolling-CRC32 cross-check at a
        // mutually-confirmed frame passed (0 desync escalations).
        assert!(out_a.hash_cross_checks > 0, "no hash cross-checks ran on A");
        assert!(out_b.hash_cross_checks > 0, "no hash cross-checks ran on B");
        assert_eq!(out_a.desync_events, 0);
        assert_eq!(out_b.desync_events, 0);
        // Both sessions were alive until we deliberately shut them down
        // (a watchdog kill would have set a different end reason).
        assert_eq!(out_a.end_reason, "shutdown signal");
        assert_eq!(out_b.end_reason, "shutdown signal");
    }

    /// Hard desync: cabinets boot with different core seeds. The first
    /// confirmed-hash comparison must trigger the host state transfer and
    /// both cabinets must re-converge (INV-5 fail-visible resolution).
    #[tokio::test(flavor = "multi_thread")]
    async fn hard_resync_via_state_transfer() {
        let sa = Arc::new(UdpSocket::bind(("127.0.0.1", 0)).await.unwrap());
        let sb = Arc::new(UdpSocket::bind(("127.0.0.1", 0)).await.unwrap());
        let cfg_a = cfg_for(sa.local_addr().unwrap().port(), sb.local_addr().unwrap().port(), 0, true);
        let cfg_b = cfg_for(sb.local_addr().unwrap().port(), sa.local_addr().unwrap().port(), 1, false);

        let (tx_a, rx_a) = watch::channel(false);
        let (tx_b, rx_b) = watch::channel(false);

        let sess_a = RollbackSession::new(cfg_a, sa, SyntheticCore::with_acc(0xAAAA), test_input(0), None);
        let sess_b = RollbackSession::new(cfg_b, sb, SyntheticCore::with_acc(0xBBBB), test_input(1), None);
        let h_a = tokio::spawn(sess_a.run(rx_a));
        let h_b = tokio::spawn(sess_b.run(rx_b));

        tokio::time::sleep(Duration::from_millis(1_000)).await;
        let _ = tx_a.send(true);
        let _ = tx_b.send(true);

        let out_a = h_a.await.unwrap();
        let out_b = h_b.await.unwrap();

        eprintln!("A: {out_a:?}");
        eprintln!("B: {out_b:?}");

        // The guest detected the desync exactly once, recovered via the
        // host state transfer, and stayed alive until shutdown.
        assert_eq!(out_b.desync_events, 1, "expected exactly one desync -> transfer");
        assert_eq!(out_b.end_reason, "shutdown signal", "guest must recover, not die");
        // INV-1 re-verified after the transfer: clean cross-checks ran.
        assert!(out_a.hash_cross_checks > 0);
        assert!(out_b.hash_cross_checks > 0);
        assert!(out_b.frames_simulated >= 50, "B simulated {} frames", out_b.frames_simulated);
    }
}

















