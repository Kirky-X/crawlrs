// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// `POST /v1/map` 请求 DTO（语义对齐孤儿 forge 文件 `presentation/forge_api/map.rs`）
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MapRequestDto {
    /// 站点 URL（提取 origin 后抓取 `{origin}/sitemap.xml`）
    pub url: String,
    /// include 白名单通配（`*`/`?`），先于 exclude 应用
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_patterns: Option<Vec<String>>,
    /// exclude 黑名单通配（`*`/`?`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_patterns: Option<Vec<String>>,
    /// 返回链接上限（缺省 1000）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// `POST /v1/map` 响应 DTO：去重后的链接（保 sitemap 出现顺序）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapResponseDto {
    pub links: Vec<String>,
}
