//! B.O.S.S. Node Discovery & Fleet Rescue Engine
//!
//! Provides:
//! 1. Subnet & ARP Table scanning to recover lost ASTRO/ORION nodes.
//! 2. Direct force-connect RPC targeting a specific node IP/hostname.
//! 3. Automatic detection of local network interfaces and host IPs.
//! 4. UDP broadcast beaconing across venue LAN & virtual networks.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use crate::models::{CreditCounters, InputJitterStats, NodeTelemetry, now_epoch_ms};
use crate::state::SharedState;

pub const ASTRO_DEFAULT_DISCOVERY_PORT: u16 = 8990;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredNode {
    pub node_id: String,
    pub ip: String,
    pub port: u16,
    pub status: String,
    pub connected: bool,
    pub hardware: Option<String>,
    pub soc: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryReport {
    pub scanned_ips: usize,
    pub arp_entries_found: usize,
    pub subnets_scanned: Vec<String>,
    pub nodes: Vec<DiscoveredNode>,
    pub duration_ms: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterfaceInfo {
    pub name: String,
    pub ip: String,
    pub subnet: String,
    pub is_default: bool,
}

/// Detects the local routable IP address of the B.O.S.S. host using the standard UDP socket trick.
pub fn get_primary_local_ip() -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // Doesn't send packets, but forces kernel to select the route interface
    socket.connect("8.8.8.8:80").or_else(|_| socket.connect("1.1.1.1:80")).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ipv4) => {
            if ipv4.is_loopback() {
                None
            } else {
                Some(ipv4)
            }
        }
        _ => None,
    }
}

/// Extracts active IPv4 entries from the system ARP cache (/proc/net/arp on Linux, `arp -a` on Windows/fallback).
pub fn read_arp_table_ips() -> Vec<Ipv4Addr> {
    let mut ips = HashSet::new();

    // 1. Check /proc/net/arp (Linux standard)
    if let Ok(content) = std::fs::read_to_string("/proc/net/arp") {
        for line in content.lines().skip(1) {
            let mut parts = line.split_whitespace();
            if let Some(ip_str) = parts.next() {
                if let Ok(ip) = ip_str.parse::<Ipv4Addr>() {
                    if !ip.is_loopback() && !ip.is_multicast() && !ip.is_broadcast() {
                        ips.insert(ip);
                    }
                }
            }
        }
    }

    // 2. Fallback to `arp -a` command output (Windows or container without /proc mounted)
    if ips.is_empty() {
        if let Ok(output) = std::process::Command::new("arp").arg("-a").output() {
            let text = String::from_utf8_lossy(&output.stdout);
            for word in text.split(|c: char| c.is_whitespace() || c == '(' || c == ')') {
                if let Ok(ip) = word.parse::<Ipv4Addr>() {
                    let octets = ip.octets();
                    // Exclude loopback, multicast, and subnet broadcasts
                    if !ip.is_loopback()
                        && !ip.is_multicast()
                        && !ip.is_broadcast()
                        && octets[3] != 255
                        && octets[0] != 224
                    {
                        ips.insert(ip);
                    }
                }
            }
        }
    }

    let mut result: Vec<Ipv4Addr> = ips.into_iter().collect();
    result.sort();
    result
}

/// Discovers candidate local subnets and interfaces.
pub fn get_network_interfaces() -> Vec<NetworkInterfaceInfo> {
    let mut list = Vec::new();
    let primary_ip = get_primary_local_ip();

    if let Some(ip) = primary_ip {
        let octets = ip.octets();
        let subnet_str = format!("{}.{}.{}.0/24", octets[0], octets[1], octets[2]);
        list.push(NetworkInterfaceInfo {
            name: "primary".to_string(),
            ip: ip.to_string(),
            subnet: subnet_str,
            is_default: true,
        });
    }

    // Common virtual network / Docker / QEMU subnets if not already present
    let common_subnets = ["172.28.0.0/24", "172.18.0.0/24", "172.17.0.0/24", "192.168.100.0/24"];
    for sub in common_subnets {
        if !list.iter().any(|iface| iface.subnet == sub) {
            list.push(NetworkInterfaceInfo {
                name: "virtual".to_string(),
                ip: sub.replace(".0/24", ".1"),
                subnet: sub.to_string(),
                is_default: false,
            });
        }
    }

    list
}

/// Probes an ASTRO node over HTTP to read identity & status.
pub async fn probe_astro_node(
    client: &Client,
    host: &str,
    port: u16,
) -> Option<DiscoveredNode> {
    let url = format!("http://{}:{}/api/v1/node/info", host, port);
    let res = client
        .get(&url)
        .timeout(Duration::from_millis(450))
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        return None;
    }

    let val = res.json::<serde_json::Value>().await.ok()?;
    let node_id = val.get("node_id")?.as_str()?.to_string();
    let status = val
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("online")
        .to_string();
    let hardware = val
        .get("hardware")
        .and_then(|h| h.as_str())
        .map(|s| s.to_string());
    let soc = val.get("soc").and_then(|s| s.as_str()).map(|s| s.to_string());
    let mode = val
        .get("mode")
        .map(|m| m.to_string());

    Some(DiscoveredNode {
        node_id,
        ip: host.to_string(),
        port,
        status,
        connected: false,
        hardware,
        soc,
        mode,
    })
}

/// Sends a connection directive to an ASTRO node, instructing it to connect to BOSS immediately.
pub async fn send_connect_directive(
    client: &Client,
    host: &str,
    port: u16,
    boss_url: &str,
    node_token: &str,
) -> Result<String, String> {
    let url = format!("http://{}:{}/api/v1/boss/connect", host, port);
    let body = serde_json::json!({
        "boss_url": boss_url,
        "token": node_token
    });

    let res = client
        .post(&url)
        .timeout(Duration::from_millis(1500))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Failed to reach ASTRO node at {host}:{port}: {e}"))?;

    if res.status().is_success() {
        let val: serde_json::Value = res.json().await.unwrap_or_default();
        let msg = val
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Connected");
        Ok(msg.to_string())
    } else {
        Err(format!("ASTRO node returned HTTP error status {}", res.status()))
    }
}

/// Helper to register an initial mock/discovered telemetry in BOSS's node map so it shows up in `/admin` instantly.
pub async fn register_initial_node_telemetry(
    state: &SharedState,
    node: &DiscoveredNode,
) {
    let now = now_epoch_ms();
    let telemetry = NodeTelemetry {
        node_id: node.node_id.clone(),
        hardware: node.hardware.clone().unwrap_or_else(|| "Generic ARM64".to_string()),
        soc: node.soc.clone().unwrap_or_else(|| "Broadcom/Rockchip".to_string()),
        cpu_temp_c: Some(42.0),
        gpu_temp_c: Some(40.0),
        npu_load_pct: 0.0,
        active_core: "none".to_string(),
        active_rom: "idle".to_string(),
        frame_time_ms: 16.66,
        fps: 60.0,
        input_jitter: InputJitterStats {
            poll_rate_hz: 1000.0,
            p50_us: 120.0,
            p95_us: 250.0,
            p99_us: 400.0,
            max_us: 650.0,
            dropped_events: 0,
        },
        netplay: None,
        credits: CreditCounters::default(),
        mode: serde_json::json!({ "type": "Standalone" }),
        timestamp_epoch_ms: now,
    };

    let mut nodes = state.nodes.write().await;
    nodes.insert(node.node_id.clone(), (telemetry, now));
}

/// Normalizes a target string (e.g. `172.28.0.3`, `172.28.0.3:8990`, `http://172.28.0.3:8990`, `astro`)
/// into (host, port).
pub fn parse_target_host_port(target: &str) -> (String, u16) {
    let clean = target
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');

    if let Some((h, p_str)) = clean.split_once(':') {
        let port = p_str.parse::<u16>().unwrap_or(ASTRO_DEFAULT_DISCOVERY_PORT);
        (h.to_string(), port)
    } else {
        (clean.to_string(), ASTRO_DEFAULT_DISCOVERY_PORT)
    }
}

/// Forces a direct connection to a specific ASTRO node.
pub async fn force_connect_node(
    target: &str,
    boss_url_override: Option<&str>,
    state: &SharedState,
) -> Result<DiscoveredNode, String> {
    let (host, port) = parse_target_host_port(target);
    if host.is_empty() {
        return Err("Target host or IP cannot be empty".to_string());
    }

    let client = Client::builder()
        .timeout(Duration::from_millis(2000))
        .build()
        .map_err(|e| e.to_string())?;

    // Determine the BOSS URL to send to ASTRO
    let boss_url = if let Some(url) = boss_url_override {
        if !url.trim().is_empty() {
            url.trim().trim_end_matches('/').to_string()
        } else {
            default_boss_callback_url()
        }
    } else {
        default_boss_callback_url()
    };

    println!("[i] Forcing ASTRO connection to {host}:{port} with BOSS callback {boss_url}");

    // 1. Probe node
    let mut node = probe_astro_node(&client, &host, port)
        .await
        .ok_or_else(|| format!("Could not reach ASTRO node discovery endpoint at {host}:{port}. Ensure ASTRO is running."))?;

    // 2. Dispatch connect directive
    send_connect_directive(&client, &host, port, &boss_url, &state.config.node_token).await?;

    node.connected = true;

    // 3. Register initial telemetry so admin console updates immediately
    register_initial_node_telemetry(state, &node).await;

    Ok(node)
}

/// Scans local subnets, ARP table, and virtual networks for responsive ASTRO nodes.
pub async fn scan_subnets_and_arp(
    boss_url_override: Option<&str>,
    custom_subnet: Option<&str>,
    state: &SharedState,
) -> DiscoveryReport {
    let start = Instant::now();
    let client = Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap_or_default();

    let boss_url = boss_url_override
        .filter(|u| !u.trim().is_empty())
        .map(|u| u.trim().trim_end_matches('/').to_string())
        .unwrap_or_else(default_boss_callback_url);

    // 1. Collect candidate IPs
    let mut candidate_ips: HashSet<String> = HashSet::new();
    let arp_ips = read_arp_table_ips();
    let arp_count = arp_ips.len();

    for ip in &arp_ips {
        candidate_ips.insert(ip.to_string());
    }

    let mut scanned_subnets = Vec::new();

    // If custom subnet specified, scan that
    if let Some(sub) = custom_subnet {
        if let Some((prefix, _)) = sub.split_once("/24") {
            let p_clean = prefix.trim_end_matches(".0").trim_end_matches('.');
            scanned_subnets.push(format!("{}.0/24", p_clean));
            for i in 1..=254 {
                candidate_ips.insert(format!("{}.{}", p_clean, i));
            }
        }
    } else {
        // Auto-detect primary subnet
        if let Some(primary_ip) = get_primary_local_ip() {
            let oct = primary_ip.octets();
            let sub_prefix = format!("{}.{}.{}", oct[0], oct[1], oct[2]);
            scanned_subnets.push(format!("{}.0/24", sub_prefix));
            for i in 1..=254 {
                candidate_ips.insert(format!("{}.{}", sub_prefix, i));
            }
        }

        // Also add standard docker/container bridge default IPs
        for docker_ip in &["172.17.0.2", "172.17.0.3", "172.18.0.2", "172.18.0.3", "172.28.0.2", "172.28.0.3"] {
            candidate_ips.insert(docker_ip.to_string());
        }
    }

    // Known standard DNS hostnames
    for name in &["astro", "astro-01", "orion-01", "localhost", "127.0.0.1"] {
        candidate_ips.insert(name.to_string());
    }

    let total_candidates = candidate_ips.len();

    // 2. Broadcast UDP Probe across LAN/broadcast
    let udp_boss_url = boss_url.clone();
    tokio::spawn(async move {
        if let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await {
            let _ = socket.set_broadcast(true);
            let msg = format!("BEERCADE_PROBE:{{\"boss_url\":\"{}\"}}", udp_boss_url);
            for bcast in &["255.255.255.255:8990", "172.28.0.255:8990", "192.168.100.255:8990"] {
                if let Ok(addr) = bcast.parse::<SocketAddr>() {
                    let _ = socket.send_to(msg.as_bytes(), addr).await;
                }
            }
        }
    });

    // 3. Concurrently probe candidate hosts (bounded concurrency with tokio Semaphore)
    let semaphore = Arc::new(tokio::sync::Semaphore::new(32));
    let mut tasks = tokio::task::JoinSet::new();

    for host in candidate_ips {
        let sem = semaphore.clone();
        let cl = client.clone();
        tasks.spawn(async move {
            let _permit = sem.acquire().await.ok()?;
            probe_astro_node(&cl, &host, ASTRO_DEFAULT_DISCOVERY_PORT).await
        });
    }

    let mut discovered: Vec<DiscoveredNode> = Vec::new();
    let mut seen_node_ids = HashSet::new();

    while let Some(res) = tasks.join_next().await {
        if let Ok(Some(mut node)) = res {
            if !seen_node_ids.contains(&node.node_id) {
                seen_node_ids.insert(node.node_id.clone());

                // Command this node to connect to BOSS
                if let Ok(_) = send_connect_directive(
                    &client,
                    &node.ip,
                    node.port,
                    &boss_url,
                    &state.config.node_token,
                )
                .await
                {
                    node.connected = true;
                    register_initial_node_telemetry(state, &node).await;
                }

                discovered.push(node);
            }
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    let message = if discovered.is_empty() {
        format!(
            "Scanned {} candidate addresses across ARP and subnets ({} ms). No lost ASTRO nodes detected.",
            total_candidates, duration_ms
        )
    } else {
        format!(
            "Discovered and linked {} ASTRO node(s) across subnets & ARP in {} ms.",
            discovered.len(),
            duration_ms
        )
    };

    DiscoveryReport {
        scanned_ips: total_candidates,
        arp_entries_found: arp_count,
        subnets_scanned: scanned_subnets,
        nodes: discovered,
        duration_ms,
        message,
    }
}

/// Computes the default BOSS callback URL (e.g. `http://<local_ip>:8989` or `http://boss:8989`).
pub fn default_boss_callback_url() -> String {
    let port = std::env::var("BOSS_PORT").unwrap_or_else(|_| "8989".to_string());
    if let Some(ip) = get_primary_local_ip() {
        format!("http://{}:{}", ip, port)
    } else {
        format!("http://boss:{}", port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::{get, post}, Json, Router};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_parse_target_host_port() {
        assert_eq!(
            parse_target_host_port("172.28.0.3"),
            ("172.28.0.3".to_string(), 8990)
        );
        assert_eq!(
            parse_target_host_port("172.28.0.3:9999"),
            ("172.28.0.3".to_string(), 9999)
        );
        assert_eq!(
            parse_target_host_port("http://astro-01:8990/"),
            ("astro-01".to_string(), 8990)
        );
        assert_eq!(
            parse_target_host_port("https://192.168.1.50/"),
            ("192.168.1.50".to_string(), 8990)
        );
    }

    #[test]
    fn test_arp_and_interfaces_no_panic() {
        let _ = read_arp_table_ips();
        let ifaces = get_network_interfaces();
        assert!(!ifaces.is_empty());
        let boss_url = default_boss_callback_url();
        assert!(boss_url.starts_with("http://"));
    }

    #[tokio::test]
    async fn test_mock_astro_probe_and_connect_directive() {
        let directive_received = Arc::new(AtomicBool::new(false));
        let flag = directive_received.clone();

        // Spawn a mock ASTRO discovery server
        let mock_app = Router::new()
            .route(
                "/api/v1/node/info",
                get(|| async {
                    Json(serde_json::json!({
                        "node_id": "MOCK-ASTRO-99",
                        "status": "online",
                        "hardware": "Radxa Rock 5C",
                        "soc": "Rockchip RK3588S",
                        "boss_url": "http://boss:8989",
                        "discovery_port": 8990,
                        "mode": { "type": "Standalone" }
                    }))
                }),
            )
            .route(
                "/api/v1/boss/connect",
                post(move |Json(payload): Json<serde_json::Value>| {
                    let flag = flag.clone();
                    async move {
                        if payload.get("boss_url").is_some() {
                            flag.store(true, Ordering::SeqCst);
                        }
                        Json(serde_json::json!({
                            "status": "ok",
                            "node_id": "MOCK-ASTRO-99",
                            "message": "Connected"
                        }))
                    }
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, mock_app).await;
        });

        let client = Client::new();
        let node = probe_astro_node(&client, "127.0.0.1", port).await;
        assert!(node.is_some());
        let n = node.unwrap();
        assert_eq!(n.node_id, "MOCK-ASTRO-99");
        assert_eq!(n.status, "online");

        let state = Arc::new(crate::state::AppState::new(crate::state::Config {
            venue_name: "Test".to_string(),
            venue_address: "123".to_string(),
            rpc_token: "secret".to_string(),
            node_token: "node_secret".to_string(),
            operator_pin: "898900".to_string(),
            edge_ingest_url: None,
            edge_ingest_token: String::new(),
        }));

        let target_str = format!("127.0.0.1:{}", port);
        let res = force_connect_node(&target_str, Some("http://127.0.0.1:8989"), &state).await;
        assert!(res.is_ok());
        let connected_node = res.unwrap();
        assert_eq!(connected_node.node_id, "MOCK-ASTRO-99");
        assert!(connected_node.connected);
        assert!(directive_received.load(Ordering::SeqCst));

        // Verify the node was registered into BOSS's node map
        let nodes = state.nodes.read().await;
        assert!(nodes.contains_key("MOCK-ASTRO-99"));
    }
}
