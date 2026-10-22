//! IPC and Network client protocol helpers connecting A.S.T.R.O. to B.O.S.S.

/// Standard REST and WebSocket endpoints exposed by B.O.S.S.
pub mod endpoints {
    pub const WS_TELEMETRY: &str = "/api/v1/ws/telemetry";
    pub const HTTP_TELEMETRY: &str = "/api/v1/telemetry";
    pub const SYNC_PAYLOAD: &str = "/api/v1/sync";
    pub const HEALTH_CHECK: &str = "/api/v1/health";
    pub const VENUE_SNAPSHOT: &str = "/api/telemetry";
}

/// Formats the WebSocket URL for telemetry streaming from a base HTTP/HTTPS URL.
pub fn format_ws_telemetry_url(boss_base_url: &str) -> String {
    let clean = boss_base_url.trim_end_matches('/');
    if clean.starts_with("http://") {
        format!("{}/api/v1/ws/telemetry", clean.replacen("http://", "ws://", 1))
    } else if clean.starts_with("https://") {
        format!("{}/api/v1/ws/telemetry", clean.replacen("https://", "wss://", 1))
    } else if clean.starts_with("ws://") || clean.starts_with("wss://") {
        format!("{}/api/v1/ws/telemetry", clean)
    } else {
        format!("ws://{}/api/v1/ws/telemetry", clean)
    }
}

/// Formats the HTTP fallback URL for telemetry posting from a base URL.
pub fn format_http_telemetry_url(boss_base_url: &str) -> String {
    let clean = boss_base_url.trim_end_matches('/');
    if clean.starts_with("ws://") {
        format!("{}/api/v1/telemetry", clean.replacen("ws://", "http://", 1))
    } else if clean.starts_with("wss://") {
        format!("{}/api/v1/telemetry", clean.replacen("wss://", "https://", 1))
    } else if clean.starts_with("http://") || clean.starts_with("https://") {
        format!("{}/api/v1/telemetry", clean)
    } else {
        format!("http://{}/api/v1/telemetry", clean)
    }
}

/// Formats the OTA sync URL for payload downloading from a base URL and payload filename.
pub fn format_ota_download_url(boss_base_url: &str, filename: &str) -> String {
    let clean = boss_base_url.trim_end_matches('/');
    let http_base = if clean.starts_with("ws://") {
        clean.replacen("ws://", "http://", 1)
    } else if clean.starts_with("wss://") {
        clean.replacen("wss://", "https://", 1)
    } else if clean.starts_with("http://") || clean.starts_with("https://") {
        clean.to_string()
    } else {
        format!("http://{}", clean)
    };
    format!("{}/api/v1/sync/{}", http_base, filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_url_formatting() {
        assert_eq!(
            format_ws_telemetry_url("http://boss.lan:8080"),
            "ws://boss.lan:8080/api/v1/ws/telemetry"
        );
        assert_eq!(
            format_ws_telemetry_url("https://boss.lan:8080/"),
            "wss://boss.lan:8080/api/v1/ws/telemetry"
        );
        assert_eq!(
            format_ws_telemetry_url("boss.lan:8080"),
            "ws://boss.lan:8080/api/v1/ws/telemetry"
        );
    }

    #[test]
    fn test_http_url_formatting() {
        assert_eq!(
            format_http_telemetry_url("ws://boss.lan:8080"),
            "http://boss.lan:8080/api/v1/telemetry"
        );
        assert_eq!(
            format_http_telemetry_url("http://boss.lan:8080/"),
            "http://boss.lan:8080/api/v1/telemetry"
        );
    }

    #[test]
    fn test_ota_download_url() {
        assert_eq!(
            format_ota_download_url("http://boss.lan:8080", "rootfs.squashfs"),
            "http://boss.lan:8080/api/v1/sync/rootfs.squashfs"
        );
    }
}
