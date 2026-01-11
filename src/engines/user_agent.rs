// Copyright (c) 2025 Kirky.X
//
// Licensed under MIT License
use rand::Rng;
// See LICENSE file in the project root for full license information.

use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Global UA rotation counter
static UA_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// User agent pool for rotation
static UA_POOL: Lazy<Vec<String>> = Lazy::new(|| {
    vec![
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36".to_string(),
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:121.0) Gecko/20100101 Firefox/121.0".to_string(),
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Safari/605.1.15".to_string(),
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Edg/120.0.0.0".to_string(),
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_14_6) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
    ]
});

/// Get rotated user agent (round-robin)
pub fn get_rotated_user_agent() -> String {
    let index = UA_COUNTER.fetch_add(1, Ordering::SeqCst) % UA_POOL.len();
    UA_POOL[index].clone()
}

/// Get random user agent
pub fn get_random_user_agent() -> String {
    let mut rng = rand::rng();
    let index = rng.random_range(0..UA_POOL.len());
    UA_POOL[index].clone()
}

/// UserAgentManager for more control over rotation
pub struct UserAgentManager {
    counter: AtomicUsize,
}

impl Default for UserAgentManager {
    fn default() -> Self {
        Self::new()
    }
}

impl UserAgentManager {
    pub fn new() -> Self {
        Self {
            counter: AtomicUsize::new(0),
        }
    }

    /// Get next user agent (round-robin)
    pub fn next(&self) -> String {
        let index = self.counter.fetch_add(1, Ordering::SeqCst) % UA_POOL.len();
        UA_POOL[index].clone()
    }

    /// Get random user agent
    pub fn random(&self) -> String {
        get_random_user_agent()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ua_pool_not_empty() {
        assert!(!UA_POOL.is_empty());
        assert!(UA_POOL.len() >= 5);
    }

    #[test]
    fn test_ua_contains_mozilla() {
        for ua in UA_POOL.iter() {
            assert!(ua.starts_with("Mozilla/"));
        }
    }

    #[test]
    fn test_rotated_ua_different() {
        let ua1 = get_rotated_user_agent();
        let ua2 = get_rotated_user_agent();
        let ua3 = get_rotated_user_agent();

        // With a pool of 8, first 3 should be different
        assert_ne!(ua1, ua2);
        assert_ne!(ua2, ua3);
    }

    #[test]
    fn test_user_agent_manager() {
        let manager = UserAgentManager::new();
        let ua1 = manager.next();
        let ua2 = manager.next();

        assert!(!ua1.is_empty());
        assert!(!ua2.is_empty());
    }
}
