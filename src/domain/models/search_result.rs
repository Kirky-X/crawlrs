// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
    pub engine: String,
    pub score: f64,
    pub published_time: Option<DateTime<Utc>>,
}

impl Default for SearchResult {
    fn default() -> Self {
        Self {
            title: String::new(),
            url: String::new(),
            description: None,
            engine: String::new(),
            score: 0.0,
            published_time: None,
        }
    }
}

impl SearchResult {
    pub fn new(title: String, url: String, description: Option<String>, engine: String) -> Self {
        Self {
            title,
            url,
            description,
            engine,
            score: 0.0,
            published_time: None,
        }
    }
}
