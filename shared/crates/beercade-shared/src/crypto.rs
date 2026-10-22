//! Cryptographic utilities for OTA payload validation and integrity verification.

use sha2::{Digest, Sha256};

/// Computes the SHA-256 digest of a byte slice, returning a lowercase hexadecimal string.
pub fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Verifies whether the SHA-256 digest of `data` matches `expected_hex` (case-insensitive).
pub fn verify_sha256(data: &[u8], expected_hex: &str) -> bool {
    let actual = compute_sha256(data);
    crate::auth::constant_time_eq(
        actual.to_ascii_lowercase().as_bytes(),
        expected_hex.trim().to_ascii_lowercase().as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256() {
        let data = b"beercade payload";
        let hash = compute_sha256(data);
        assert!(verify_sha256(data, &hash));
        assert!(!verify_sha256(b"corrupt", &hash));
    }
}
