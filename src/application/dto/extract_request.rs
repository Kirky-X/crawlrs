// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct ExtractRequestDto {
    pub urls: Vec<String>,
    pub prompt: Option<String>,
    pub schema: Option<Value>,
    pub model: Option<String>,
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
