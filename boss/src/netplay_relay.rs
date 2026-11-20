//! ============================================================================
//! B.O.S.S. NETPLAY DISCOVERY RELAY & TELEMETRY ARBITER
//! ============================================================================
//! Role in the A.S.T.R.O. rollback netplay architecture:
//!
//!   1. DISCOVERY: cabinets ANNOUNCE over UDP; the relay pairs two stations
//!      into a session and hands each side its peer's address (so they can
//!      talk P2P over the venue's gigabit LAN).
//!   2. FALLBACK FORWARDING: if direct P2P fails, packets arriving at the
//!      relay are forwarded to the session's other endpoint (up to 1400 B
//!      datagrams at 60 Hz per side — trivial for B.O.S.S.).
//!   3. TELEMETRY ARBITER: parses WATCHDOG and DESYNC_REPORT message types
//!      (without re-encrypting/re-encoding anything) to aggregate health
//!      per session, exposes HTTP endpoints for dashboards/SSE, and counts
//!      desync escalations per session.
//!
//! Wire format mirrors astro-daemon/src/netplay.rs (magic "OR", version,
//! type, u64 session id); the relay parses ONLY the header plus the small
//! WATCHDOG/DESYNC bodies and forwards everything else byte-opaque.
//! ============================================================================

use axum::{extract::State, http::StatusCode, routing::{get, post}, Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

// --- wire constants (MIRROR of astro-daemon netplay.rs — keep in sync) -----
const MAGIC: [u8; 2] = [0x4F, 0x52]; // "OR"
const HEADER_LEN: usize = 12;
#[allow(dead_code)] // full protocol mirror — relay parses a subset
const MSG_INPUT: u8 = 0x01;
#[allow(dead_code)] // full protocol mirror — relay parses a subset
const MSG_PING: u8 = 0x02;
#[allow(dead_code)]
const MSG_PONG: u8 = 0x03;
const MSG_DESYNC_REPORT: u8 = 0x04;
#[allow(dead_code)]
const MSG_STATE_CHUNK: u8 = 0x05;
const MSG_WATCHDOG: u8 = 0x06;
const MSG_ANNOUNCE: u8 = 0x07;
const MSG_ANNOUNCE_ACK: u8 = 0x08;
const MAX_DATAGRAM: usize = 1400;

// --- session state ----------------------------------------------------------

/// Aggregated health of one cabinet inside a netplay session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EndpointHealth {
    pub node_id: String,
    pub addr: String,
    pub state: String,
    pub loss_pct: f32,
    pub last_seen_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: u64,
    pub endpoints: Vec<EndpointHealth>,
    pub desync_events: u32,
    pub created_ms: u64,
    pub last_seen_ms: u64,
    pub relayed_packets: u64,
}

struct RelayEndpoint {
    node_id: String,
    addr: SocketAddr,
    state: String,
    loss_pct: f32,
    last_seen_ms: u64,
}

pub struct RelaySession {
    endpoints: [Option<RelayEndpoint>; 2],
    pub desync_events: u32,
    pub created_ms: u64,
    pub last_seen_ms: u64,
    pub relayed_packets: u64,
}

impl RelaySession {
    fn new(now: u64) -> Self {
        RelaySession {
            endpoints: [None, None],
            desync_events: 0,
            created_ms: now,
            last_seen_ms: now,
            relayed_packets: 0,
        }
    }

    /// Slot index for an address: existing slot by addr, else first free.
    fn slot_of(&self, addr: SocketAddr) -> Option<usize> {
        if let Some(i) = self
            .endpoints
            .iter()
            .position(|e| matches!(e, Some(ep) if ep.addr == addr))
        {
            return Some(i);
        }
        self.endpoints.iter().position(|e| e.is_none())
    }

    fn peer_addr_of(&self, slot: usize) -> Option<SocketAddr> {
        self.endpoints
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != slot)
            .filter_map(|(_, e)| e.as_ref())
            .map(|e| e.addr)
            .next()
    }

    fn summary(&self, session_id: u64) -> SessionSummary {
        SessionSummary {
            session_id,
            desync_events: self.desync_events,
            created_ms: self.created_ms,
            last_seen_ms: self.last_seen_ms,
            relayed_packets: self.relayed_packets,
            endpoints: self
                .endpoints
                .iter()
                .enumerate()
                .filter_map(|(i, e)| {
                    e.as_ref().map(|ep| EndpointHealth {
                        node_id: ep.node_id.clone(),
                        addr: ep.addr.to_string(),
                        state: ep.state.clone(),
                        loss_pct: ep.loss_pct,
                        last_seen_ms: ep.last_seen_ms,
                    })
                    .map(|mut h| {
                        h.node_id = if h.node_id.is_empty() {
                            format!("slot-{i}")
                        } else {
                            h.node_id
                        };
                        h
                    })
                })
                .collect(),
        }
    }
}

pub struct RelayState {
    pub sessions: RwLock<HashMap<u64, RelaySession>>,
}

impl RelayState {
    pub fn new() -> Self {
        RelayState { sessions: RwLock::new(HashMap::new()) }
    }
}

impl Default for RelayState {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// --- UDP relay loop ----------------------------------------------------------

/// Bind the relay UDP socket and spawn the forwarding task. Returns the
/// bound address so tests and the discovery table can reference it.
pub async fn bind_udp_relay(state: Arc<RelayState>, bind: &str) -> std::io::Result<SocketAddr> {
    let socket = UdpSocket::bind(bind).await?;
    let addr = socket.local_addr()?;
    tokio::spawn(relay_loop(state, socket));
    info!("netplay relay listening on udp://{addr}");
    Ok(addr)
}

async fn relay_loop(state: Arc<RelayState>, socket: UdpSocket) {
    let mut buf = [0u8; MAX_DATAGRAM];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, from)) => {
                if let Err(e) = handle_datagram(&state, &socket, len, &buf, from).await {
                    debug!(error = %e, "relay datagram dropped");
                }
            }
            Err(e) => {
                warn!(error = %e, "relay recv failed");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

fn parse_header(buf: &[u8]) -> Option<(u64, u8)> {
    if buf.len() < HEADER_LEN || buf[0] != MAGIC[0] || buf[1] != MAGIC[1] {
        return None;
    }
    let session_id = u64::from_le_bytes(buf[4..12].try_into().ok()?);
    Some((session_id, buf[3]))
}

async fn handle_datagram(
    state: &Arc<RelayState>,
    socket: &UdpSocket,
    len: usize,
    buf: &[u8],
    from: SocketAddr,
) -> Result<(), &'static str> {
    let Some((session_id, msg_type)) = parse_header(&buf[..len]) else {
        return Err("bad header");
    };
    let now = now_ms();

    let mut sessions = state.sessions.write().await;
    let session = sessions.entry(session_id).or_insert_with(|| {
        info!(session_id, "new netplay session registered at relay");
        RelaySession::new(now)
    });
    session.last_seen_ms = now;

    if msg_type == MSG_ANNOUNCE {
        // Register/refresh the announcing endpoint and ACK with its slot
        // and (if known) the peer's address for direct P2P.
        let slot = session.slot_of(from).unwrap_or(0);
        let node_id = parse_announce_node_id(&buf[..len]).unwrap_or_default();
        let peer = session.peer_addr_of(slot);
        let is_new = session.endpoints[slot].is_none();
        session.endpoints[slot] = Some(RelayEndpoint {
            node_id,
            addr: from,
            state: "Active".to_string(),
            loss_pct: 0.0,
            last_seen_ms: now,
        });
        info!(
            session_id,
            slot,
            %from,
            peer = peer.map(|p| p.to_string()).unwrap_or_else(|| "pending".into()),
            new = is_new,
            "announced"
        );
        drop(sessions);
        let ack = build_announce_ack(slot as u8, peer);
        let _ = socket.send_to(&ack, from).await;
        return Ok(());
    }

    // Locate the sender's slot (announced or self-learned) and forward.
    let Some(slot) = session.slot_of(from) else {
        return Err("sender not registered");
    };
    session.relayed_packets += 1;

    match msg_type {
        MSG_WATCHDOG => {
            // [0]state u8, [1..3] loss x100, [3..5] cpu x100, [5..7] gpu x100
            let body = &buf[HEADER_LEN..len];
            if body.len() >= 7 {
                let st = match body[0] {
                    1 => "Degraded",
                    2 => "ThermalHold",
                    3 => "LocalMode",
                    _ => "Active",
                };
                let loss = u16::from_le_bytes([body[1], body[2]]) as f32 / 100.0;
                if let Some(ep) = &mut session.endpoints[slot] {
                    ep.state = st.to_string();
                    ep.loss_pct = loss;
                    ep.last_seen_ms = now;
                }
            }
        }
        MSG_DESYNC_REPORT => {
            session.desync_events += 1;
            warn!(
                session_id,
                desync_events = session.desync_events,
                "arbiter: desync escalation observed in session"
            );
        }
        _ => {}
    }

    if let Some(peer) = session.peer_addr_of(slot) {
        // Opaque fallback forwarding (discovery relay path).
        let _ = socket.send_to(&buf[..len], peer).await;
    }
    Ok(())
}

fn parse_announce_node_id(buf: &[u8]) -> Option<String> {
    if buf.len() < HEADER_LEN + 1 {
        return None;
    }
    let l = buf[HEADER_LEN] as usize;
    if buf.len() < HEADER_LEN + 1 + l {
        return None;
    }
    Some(String::from_utf8_lossy(&buf[HEADER_LEN + 1..HEADER_LEN + 1 + l]).into_owned())
}

fn build_announce_ack(slot: u8, peer: Option<SocketAddr>) -> Vec<u8> {
    let mut b = Vec::with_capacity(24);
    b.extend_from_slice(&MAGIC);
    b.push(1); // version
    b.push(MSG_ANNOUNCE_ACK);
    b.extend_from_slice(&0u64.to_le_bytes()); // session ack uses id 0 on wire
    b.push(slot);
    match peer {
        Some(SocketAddr::V4(a)) => {
            b.push(4);
            b.extend_from_slice(&u32::from_ne_bytes(a.ip().octets()).to_le_bytes());
            b.extend_from_slice(&a.port().to_le_bytes());
        }
        _ => b.push(0),
    }
    b
}

// --- HTTP API (mounted into the existing B.O.S.S. axum router) ----------------

/// GET /api/v1/netplay/sessions — dashboard-visible session table.
async fn list_sessions(State(state): State<Arc<RelayState>>) -> Json<Vec<SessionSummary>> {
    let mut sessions = state.sessions.write().await;
    let now = now_ms();
    // GC: drop sessions silent for over 5 minutes.
    sessions.retain(|_, s| now.saturating_sub(s.last_seen_ms) < 300_000);
    let mut list: Vec<SessionSummary> = sessions
        .iter()
        .map(|(id, s)| s.summary(*id))
        .collect();
    list.sort_by_key(|s| s.session_id);
    Json(list)
}

/// POST /api/v1/netplay/report — HTTP telemetry ingest from astro-daemon
/// watchdog transitions (the NetplayHealthReport JSON body).
#[derive(Debug, Deserialize)]
struct NetplayReportBody {
    session_id: u64,
    node_id: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    loss_pct: Option<f32>,
    #[serde(default)]
    desync_events: Option<u32>,
    #[serde(default)]
    reason: Option<String>,
}

async fn ingest_report(
    State(state): State<Arc<RelayState>>,
    Json(body): Json<NetplayReportBody>,
) -> StatusCode {
    let now = now_ms();
    let mut sessions = state.sessions.write().await;
    let session = sessions.entry(body.session_id).or_insert_with(|| RelaySession::new(now));
    session.last_seen_ms = now;
    // Attribute the report to a slot by node_id (or first free slot).
    let slot = session
        .endpoints
        .iter()
        .position(|e| matches!(e, Some(ep) if ep.node_id == body.node_id))
        .unwrap_or(0);
    let ep = session.endpoints[slot].get_or_insert_with(|| RelayEndpoint {
        node_id: body.node_id.clone(),
        addr: "unknown".parse().unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap()),
        state: "Active".to_string(),
        loss_pct: 0.0,
        last_seen_ms: now,
    });
    ep.last_seen_ms = now;
    if let Some(st) = &body.state {
        ep.state = st.clone();
    }
    if let Some(loss) = body.loss_pct {
        ep.loss_pct = loss;
    }
    if let Some(d) = body.desync_events {
        if d > session.desync_events {
            session.desync_events = d;
        }
    }
    info!(
        session_id = body.session_id,
        node = %body.node_id,
        state = ep.state,
        reason = body.reason.as_deref().unwrap_or(""),
        "netplay health report"
    );
    StatusCode::OK
}

/// Router fragment to merge into B.O.S.S.'s main Router.
pub fn router(state: Arc<RelayState>) -> Router {
    Router::new()
        .route("/api/v1/netplay/sessions", get(list_sessions))
        .route("/api/v1/netplay/report", post(ingest_report))
        .with_state(state)
}

// --- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn announce(session_id: u64, node_id: &str) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&MAGIC);
        b.push(1);
        b.push(MSG_ANNOUNCE);
        b.extend_from_slice(&session_id.to_le_bytes());
        b.push(node_id.len() as u8);
        b.extend_from_slice(node_id.as_bytes());
        b
    }

    fn input_packet(session_id: u64) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&MAGIC);
        b.push(1);
        b.push(MSG_INPUT);
        b.extend_from_slice(&session_id.to_le_bytes());
        b.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        b
    }

    /// Discovery handshake: two stations announce, each learns its slot and
    /// the peer's address; then fallback forwarding carries INPUT frames.
    #[tokio::test(flavor = "multi_thread")]
    async fn relay_discovery_and_forwarding() {
        let state = Arc::new(RelayState::new());
        let relay_addr = bind_udp_relay(state.clone(), "127.0.0.1:0").await.unwrap();

        let sa = UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
        let sb = UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
        sa.connect(relay_addr).await.unwrap();
        sb.connect(relay_addr).await.unwrap();

        // Station A announces.
        sa.send(&announce(0xCAFE_0002, "ORION-01")).await.unwrap();
        let mut ack_a = [0u8; 64];
        let (n, _) = timeout_recv(&sa, &mut ack_a).await;
        // slot(1) + no-peer family byte(1): 14 bytes total.
        assert_eq!(n, HEADER_LEN + 2, "ack layout for pending peer");
        assert_eq!(ack_a[HEADER_LEN], 0, "first announcer gets slot 0");

        // Station B announces; its ACK carries A's address for direct P2P.
        sb.send(&announce(0xCAFE_0002, "ORION-02")).await.unwrap();
        let mut ack_b = [0u8; 64];
        let (n, _) = timeout_recv(&sb, &mut ack_b).await;
        assert_eq!(ack_b[HEADER_LEN], 1, "second announcer gets slot 1");
        assert!(n > HEADER_LEN + 1 + 5, "B's ack must embed A's address");

        // A sends an INPUT datagram through the relay; B receives it.
        sa.send(&input_packet(0xCAFE_0002)).await.unwrap();
        let mut rx = [0u8; MAX_DATAGRAM];
        let (n, _) = timeout_recv(&sb, &mut rx).await;
        assert_eq!(&rx[..n], &input_packet(0xCAFE_0002)[..], "forwarded opaquely");

        // Session table reflects both endpoints + relay counters.
        let sessions = state.sessions.read().await;
        let s = sessions.get(&0xCAFE_0002).expect("session registered");
        assert_eq!(s.summary(0xCAFE_0002).endpoints.len(), 2);
        assert_eq!(s.relayed_packets, 1);
    }

    async fn timeout_recv(sock: &UdpSocket, buf: &mut [u8]) -> (usize, SocketAddr) {
        let n = tokio::time::timeout(Duration::from_secs(2), sock.recv_from(buf))
            .await
            .expect("recv timeout")
            .unwrap();
        n
    }

    /// HTTP telemetry arbiter: health reports update the session table.
    #[tokio::test(flavor = "multi_thread")]
    async fn relay_health_report_ingest() {
        let state = Arc::new(RelayState::new());
        let app = router(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let body = serde_json::json!({
            "session_id": 77,
            "node_id": "ORION-01",
            "state": "Degraded",
            "loss_pct": 6.25,
            "desync_events": 3,
            "reason": "packet loss beyond 5% EWMA"
        })
        .to_string();

        // Minimal HTTP POST without an external client dependency.
        let req = format!(
            "POST /api/v1/netplay/report HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let status = http_roundtrip(addr, &req).await;
        assert!(status.contains("200 OK"), "report ingest failed: {status}");

        let req = format!(
            "GET /api/v1/netplay/sessions HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
        );
        let resp = http_roundtrip(addr, &req).await;
        assert!(resp.contains("200 OK"), "sessions listing failed");
        let json_start = resp.find('[').expect("json body");
        let json_end = resp.rfind(']').expect("json body end") + 1;
        let sessions: Vec<SessionSummary> =
            serde_json::from_str(&resp[json_start..json_end]).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, 77);
        assert_eq!(sessions[0].desync_events, 3);
        assert_eq!(sessions[0].endpoints[0].node_id, "ORION-01");
        assert_eq!(sessions[0].endpoints[0].state, "Degraded");
        assert!((sessions[0].endpoints[0].loss_pct - 6.25).abs() < 0.01);
    }

    /// Raw-socket HTTP/1.1 roundtrip; returns the status line + body.
    async fn http_roundtrip(addr: SocketAddr, request: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        sock.write_all(request.as_bytes()).await.unwrap();
        let mut resp = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(2), sock.read_to_end(&mut resp)).await;
        String::from_utf8_lossy(&resp).into_owned()
    }
}



