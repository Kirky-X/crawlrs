// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Shared types for FlareSolverr-based engines
//!
//! This module consolidates the FlareSolverr request/response structures
//! that were previously duplicated across multiple engine implementations.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Base FlareSolverr request structure shared across all engines
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FlareSolverrRequest {
    /// Command to execute (e.g., "request.get")
    pub cmd: String,
    /// Target URL to scrape
    pub url: String,
    /// Maximum timeout in milliseconds
    #[serde(rename = "maxTimeout")]
    pub max_timeout: u64,
    /// Whether to return screenshots
    #[serde(rename = "returnScreenshot", skip_serializing_if = "Option::is_none")]
    pub return_screenshot: Option<bool>,
    /// Custom headers to include in request
    #[serde(rename = "customHeaders", skip_serializing_if = "Option::is_none")]
    pub custom_headers: Option<HashMap<String, String>>,
    /// Session ID for persistent sessions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

impl FlareSolverrRequest {
    /// Create a new base request
    pub fn new(cmd: String, url: String, max_timeout: u64) -> Self {
        Self {
            cmd,
            url,
            max_timeout,
            return_screenshot: None,
            custom_headers: None,
            session: None,
        }
    }

    /// Set screenshot requirement
    pub fn with_screenshot(mut self, return_screenshot: bool) -> Self {
        self.return_screenshot = Some(return_screenshot);
        self
    }

    /// Set custom headers
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.custom_headers = Some(headers);
        self
    }

    /// Set session ID
    pub fn with_session(mut self, session: String) -> Self {
        self.session = Some(session);
        self
    }
}

/// FlareSolverr response structure
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FlareSolverrResponse {
    /// Response status
    pub status: String,
    /// Response message
    pub message: String,
    /// Solution data if successful
    pub solution: Option<FlareSolverrSolution>,
    /// Request start timestamp
    #[serde(rename = "startTimestamp")]
    #[allow(dead_code)]
    pub start_timestamp: u64,
    /// Request end timestamp
    #[serde(rename = "endTimestamp")]
    #[allow(dead_code)]
    pub end_timestamp: u64,
    /// FlareSolverr version
    #[allow(dead_code)]
    pub version: String,
}

/// Solution data from FlareSolverr
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FlareSolverrSolution {
    /// Response URL
    pub url: String,
    /// Response status code
    pub status: u16,
    /// Response headers
    #[allow(dead_code)]
    pub headers: HashMap<String, String>,
    /// Response cookies
    #[allow(dead_code)]
    pub cookies: Vec<HashMap<String, String>>,
    /// Response content (base64 encoded if binary)
    pub response: String,
    /// Whether response is base64 encoded
    #[serde(rename = "responseType")]
    #[allow(dead_code)]
    pub response_type: Option<String>,
}
