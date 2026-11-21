//! Cryptographic utilities for OTA payload validation, discovery HMAC, and integrity verification.

use sha2::{Digest, Sha256};

const HMAC_BLOCK_SIZE: usize = 64;

/// Computes the SHA-256 digest of a byte slice, returning a lowercase hexadecimal string.
pub fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256Hasher::new();
    hasher.update(data);
    hasher.finalize_hex()
}

/// Verifies whether the SHA-256 digest of `data` matches `expected_hex` (case-insensitive).
pub fn verify_sha256(data: &[u8], expected_hex: &str) -> bool {
    let mut hasher = Sha256Hasher::new();
    hasher.update(data);
    hasher.verify_hex(expected_hex)
}

/// Incremental SHA-256 hasher producing 64-character lowercase hexadecimal digests.
#[derive(Debug, Clone)]
pub struct Sha256Hasher {
    inner: Sha256,
}

impl Sha256Hasher {
    /// Creates a new SHA-256 hasher.
    pub fn new() -> Self {
        Self {
            inner: Sha256::new(),
        }
    }

    /// Feeds `data` into the digest state.
    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    /// Finalizes the digest and returns a 64-character lowercase hex string.
    pub fn finalize_hex(self) -> String {
        format!("{:x}", self.inner.finalize())
    }

    /// Finalizes the digest and checks it against `expected_hex` in constant time.
    pub fn verify_hex(self, expected_hex: &str) -> bool {
        let actual = self.finalize_hex();
        verify_sha256_hex(&actual, expected_hex)
    }
}

impl Default for Sha256Hasher {
    fn default() -> Self {
        Self::new()
    }
}

/// Computes the SHA-256 digest of a reader by streaming in 64 KiB chunks.
pub fn compute_reader_sha256<R: std::io::Read>(mut reader: R) -> std::io::Result<String> {
    let mut hasher = Sha256Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize_hex())
}

/// Verifies two hex-encoded SHA-256 digests in constant time (trimmed, case-insensitive, length == 64).
pub fn verify_sha256_hex(actual_hex: &str, expected_hex: &str) -> bool {
    let actual_norm = actual_hex.trim().to_ascii_lowercase();
    let expected_norm = expected_hex.trim().to_ascii_lowercase();
    if actual_norm.len() != 64 || expected_norm.len() != 64 {
        return false;
    }
    crate::auth::constant_time_eq(actual_norm.as_bytes(), expected_norm.as_bytes())
}

/// Computes RFC 2104 HMAC-SHA256 over `msg` using `key`.
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut key_block = [0u8; HMAC_BLOCK_SIZE];
    if key.len() > HMAC_BLOCK_SIZE {
        let digest = Sha256::digest(key);
        key_block[..32].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; HMAC_BLOCK_SIZE];
    let mut opad = [0x5cu8; HMAC_BLOCK_SIZE];
    for i in 0..HMAC_BLOCK_SIZE {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(msg);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    outer.finalize().into()
}

/// Computes RFC 2104 HMAC-SHA256 and returns lowercase 64-char hex.
pub fn hmac_sha256_hex(key: &[u8], msg: &[u8]) -> String {
    let tag = hmac_sha256(key, msg);
    let mut out = String::with_capacity(64);
    for b in tag {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Verifies an expected lowercase/uppercase hex HMAC-SHA256 tag in constant time.
pub fn verify_hmac_sha256_hex(key: &[u8], msg: &[u8], expected_hex: &str) -> bool {
    let expected_lower = expected_hex.trim().to_ascii_lowercase();
    if expected_lower.len() != 64 {
        return false;
    }
    let actual = hmac_sha256_hex(key, msg);
    crate::auth::constant_time_eq(actual.as_bytes(), expected_lower.as_bytes())
}

/// Derives a domain-separated 32-byte discovery key from a provisioned shared secret/token.
pub fn derive_discovery_key(secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"beercade-discovery-v1:");
    hasher.update(secret.as_bytes());
    hasher.finalize().into()
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
        assert!(!verify_sha256(data, "short"));
    }

    #[test]
    fn test_sha256_hasher_streaming_and_reader() {
        // Multi-chunk payload spanning > 64 KiB to exercise multiple read iterations.
        let mut payload = Vec::with_capacity(140 * 1024);
        for i in 0..(140 * 1024) {
            payload.push((i % 251) as u8);
        }

        let expected = compute_sha256(&payload);
        assert_eq!(expected.len(), 64);

        let mut hasher = Sha256Hasher::default();
        for chunk in payload.chunks(4099) {
            hasher.update(chunk);
        }
        assert!(hasher.clone().verify_hex(&expected.to_ascii_uppercase()));
        assert!(!hasher.clone().verify_hex(&"0".repeat(64)));
        assert_eq!(hasher.finalize_hex(), expected);

        let reader_hex = compute_reader_sha256(std::io::Cursor::new(&payload)).unwrap();
        assert_eq!(reader_hex, expected);

        assert!(verify_sha256_hex(&expected, &format!("  {}  ", expected.to_ascii_uppercase())));
        assert!(!verify_sha256_hex("deadbeef", "deadbeef"));
        assert!(!verify_sha256_hex(&expected, &"a".repeat(63)));
    }

    #[test]
    fn test_hmac_sha256_rfc4231_vectors() {
        // RFC 4231 Test Case 1
        let key1 = [0x0bu8; 20];
        let data1 = b"Hi There";
        let expected1 = "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7";
        assert_eq!(hmac_sha256_hex(&key1, data1), expected1);
        assert!(verify_hmac_sha256_hex(&key1, data1, expected1));
        assert!(verify_hmac_sha256_hex(&key1, data1, &expected1.to_ascii_uppercase()));

        // RFC 4231 Test Case 2
        let key2 = b"Jefe";
        let data2 = b"what do ya want for nothing?";
        let expected2 = "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843";
        assert_eq!(hmac_sha256_hex(key2, data2), expected2);
        assert!(verify_hmac_sha256_hex(key2, data2, expected2));

        // Negative cases: tampered message, wrong key, wrong length
        assert!(!verify_hmac_sha256_hex(key2, b"tampered", expected2));
        assert!(!verify_hmac_sha256_hex(b"wrong", data2, expected2));
        assert!(!verify_hmac_sha256_hex(key2, data2, "deadbeef"));
    }
}
