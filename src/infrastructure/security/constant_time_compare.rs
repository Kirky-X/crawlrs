// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Constant-time string comparison helper
//!
//! Provides `constant_time_eq_str` to mitigate timing side-channel attacks
//! when comparing fixed-length secret-derived strings (e.g. SHA-256 hex
//! digests of API keys).
//!
//! # Security
//!
//! `==` on `&str` performs byte-by-byte comparison with short-circuit
//! semantics: it returns `false` on the first mismatched byte. An attacker
//! who can measure response time can recover the secret digest byte-by-byte
//! by sending crafted inputs and observing whether the comparison takes
//! slightly longer for the correct prefix.
//!
//! `constant_time_eq_str` uses [`subtle::ConstantTimeEq`] (`ct_eq`) which
//! guarantees every byte participates in the comparison before the result
//! is returned, regardless of where the first mismatch occurs. This removes
//! the timing signal exploited by the attack above.
//!
//! # Limitations
//!
//! - **Length check is NOT constant-time.** When `a.len() != b.len()` the
//!   function returns `false` immediately. This is acceptable for the
//!   current call sites because the expected length is fixed and publicly
//!   known (SHA-256 hex digest = 64 ASCII chars); length is not a secret.
//!   If you need constant-time comparison for variable-length secrets,
//!   pad both inputs to a fixed length before calling this function.
//! - **Only `&[u8]` semantics.** The function compares the UTF-8 byte
//!   representation of the input strings; normalization is the caller's
//!   responsibility.
//! - **Not a replacement for `bcrypt::verify`.** For password / API key
//!   verification against a bcrypt hash (`$2b$...`), use
//!   [`crate::infrastructure::security::verify_api_key`] directly — bcrypt
//!   already performs constant-time comparison internally.
//!
//! # When to use
//!
//! Use this helper only when comparing two fixed-length, secret-derived
//! strings (e.g. SHA-256 hex digests). For non-secret comparisons (URLs,
//! user IDs, configuration values), prefer `==` — the constant-time
//! overhead is unnecessary and reduces readability.

use subtle::ConstantTimeEq;

/// Constant-time comparison of two equal-length strings.
///
/// Returns `false` immediately if `a.len() != b.len()` (length is not a
/// secret — see module docs). Otherwise performs a constant-time byte
/// comparison via `subtle::ConstantTimeEq::ct_eq`.
///
/// # Security
///
/// Mitigates timing side-channel leakage of secret digest content during
/// equality comparison. See the module-level documentation for the full
/// security model and limitations.
///
/// # Arguments
///
/// * `a` - First string (typically the stored digest)
/// * `b` - Second string (typically the computed digest)
///
/// # Returns
///
/// * `true` if both strings have equal length and identical bytes
/// * `false` otherwise
pub fn constant_time_eq_str(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_equal_strings_return_true() {
        assert!(constant_time_eq_str("abc123def456", "abc123def456"));
    }

    #[test]
    fn test_different_strings_return_false() {
        assert!(!constant_time_eq_str("abc123def456", "abc123def457"));
    }

    #[test]
    fn test_different_lengths_return_false() {
        assert!(!constant_time_eq_str("abc", "abcd"));
        assert!(!constant_time_eq_str("abcd", "abc"));
    }

    #[test]
    fn test_empty_strings_return_true() {
        assert!(constant_time_eq_str("", ""));
    }

    #[test]
    fn test_empty_vs_nonempty_returns_false() {
        assert!(!constant_time_eq_str("", "a"));
        assert!(!constant_time_eq_str("a", ""));
    }

    #[test]
    fn test_sha256_hex_digest_length() {
        // Realistic SHA-256 hex digest (64 chars) — the primary use case.
        let stored = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let input = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert!(constant_time_eq_str(stored, input));

        let tampered = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b856";
        assert!(!constant_time_eq_str(stored, tampered));
    }

    #[test]
    fn test_unicode_byte_comparison() {
        // UTF-8 byte-level comparison; no normalization.
        assert!(constant_time_eq_str("héllo", "héllo"));
        assert!(!constant_time_eq_str("héllo", "hello"));
    }
}
