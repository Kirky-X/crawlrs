// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Implementation of [`FeatureFlag`].

use super::FeatureFlag;
use uuid::Uuid;

impl FeatureFlag {
    /// Check if the feature is currently active
    pub fn is_active(&self) -> bool {
        self.enabled
            && self.started_at.is_none_or(|t| t <= chrono::Utc::now())
            && self.stopped_at.is_none_or(|t| t > chrono::Utc::now())
    }

    /// Check if a specific API Key should have access based on rollout
    pub fn should_enable_for_key(&self, api_key_id: Uuid) -> bool {
        if !self.is_active() {
            return false;
        }

        if self.rollout_percentage == 100 {
            return true;
        }

        if self.rollout_percentage == 0 {
            return false;
        }

        // Deterministic rollout based on API Key ID
        let bytes = api_key_id.as_bytes();
        let mut hash: u64 = 0;
        for &byte in bytes {
            hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
        }
        let bucket = hash % 100;
        bucket < self.rollout_percentage as u64
    }
}
