// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use validator::Validate;

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct ExtractRequestDto {
    pub urls: Vec<String>,
    pub prompt: Option<String>,
    pub schema: Option<Value>,
    pub model: Option<String>,
    /// 同步等待时长（毫秒，默认 5000，最大 30000）
    #[validate(range(min = 0, max = 30000))]
    pub sync_wait_ms: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExtractResponseDto {
    pub results: Vec<ExtractResultDto>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExtractResultDto {
    pub url: String,
    pub data: Value,
    pub error: Option<String>,
}
