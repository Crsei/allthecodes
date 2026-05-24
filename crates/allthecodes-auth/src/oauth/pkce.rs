//! PKCE (Proof Key for Code Exchange) utilities for OAuth 2.0.

use base64::{engine::general_purpose::STANDARD, Engine};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Base64url-encode bytes (RFC 4648 §5, no padding).
fn base64url_encode(bytes: &[u8]) -> String {
    STANDARD
        .encode(bytes)
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_string()
}

/// Generate a cryptographically random code verifier (43 chars, base64url).
pub fn generate_code_verifier() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    base64url_encode(&buf)
}

/// Generate a code challenge from a verifier (SHA-256 → base64url).
pub fn generate_code_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    base64url_encode(&hash)
}

/// Generate a random state parameter for CSRF protection.
pub fn generate_state() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    base64url_encode(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64url_no_special_chars() {
        let bytes = [0xFF, 0xFE, 0xFD, 0xFC, 0xFB, 0xFA];
        let encoded = base64url_encode(&bytes);
        assert!(!encoded.contains('+'), "must not contain +");
        assert!(!encoded.contains('/'), "must not contain /");
        assert!(!encoded.contains('='), "must not contain =");
    }

    #[test]
    fn test_verifier_length() {
        let verifier = generate_code_verifier();
        assert_eq!(verifier.len(), 43, "32 bytes → 43 base64url chars");
    }

    #[test]
    fn test_challenge_is_sha256_of_verifier() {
        let verifier = generate_code_verifier();
        let challenge = generate_code_challenge(&verifier);
        let hash = Sha256::digest(verifier.as_bytes());
        let expected = base64url_encode(&hash);
        assert_eq!(challenge, expected);
    }

    #[test]
    fn test_state_not_empty_and_unique() {
        let s1 = generate_state();
        let s2 = generate_state();
        assert!(!s1.is_empty());
        assert_ne!(s1, s2, "consecutive states must differ");
    }
}
