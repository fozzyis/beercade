//! Shared Bearer-token authentication and credential verification protocols.

/// Length-independent byte comparison to prevent timing side-channel attacks.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Helper to validate a presented bearer token against an expected secret.
pub fn validate_token(presented: &str, expected: &str) -> bool {
    if presented.is_empty() || expected.is_empty() {
        return false;
    }
    constant_time_eq(presented.as_bytes(), expected.as_bytes())
}

/// Extracts the token from an `Authorization: Bearer <token>` string.
pub fn extract_bearer_token(header_val: &str) -> Option<&str> {
    header_val
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// Formats an `Authorization` header value with Bearer scheme.
pub fn format_bearer_header(token: &str) -> String {
    format!("Bearer {}", token)
}

/// Token claim indicating caller permission level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeClaim {
    /// `true` when caller presented operator-level RPC credential.
    pub is_operator: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secret1"));
        assert!(!constant_time_eq(b"secret1", b"secret"));
        assert!(!constant_time_eq(b"secred", b"secret"));
    }

    #[test]
    fn test_validate_token() {
        assert!(validate_token("abc123xyz", "abc123xyz"));
        assert!(!validate_token("abc123xyz", "wrong"));
        assert!(!validate_token("", "expected"));
        assert!(!validate_token("presented", ""));
    }

    #[test]
    fn test_bearer_helpers() {
        assert_eq!(extract_bearer_token("Bearer secret_tok"), Some("secret_tok"));
        assert_eq!(extract_bearer_token("Basic secret_tok"), None);
        assert_eq!(extract_bearer_token("Bearer "), None);
        assert_eq!(format_bearer_header("secret_tok"), "Bearer secret_tok");
    }
}
