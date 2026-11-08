// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Exa MCP 文本结果字段解析 — 从 `exa.rs` 拆分
//!
//! 解析结果块内的 `Title:` / `URL:` 元数据行与描述片段。

/// 解析结果块内的字段行（Title:/URL:/Published:/Author:/Highlights:）与描述片段。
pub(super) fn parse_text_fields(block: &str) -> (String, String, String) {
    let (title, url) = parse_text_metadata(block);
    let mut description = collect_description(block);

    // 截断描述到合理长度（char 边界安全，中文描述不 panic）
    if description.chars().count() > 500 {
        description = crate::common::text_slice::truncate_chars(&description, 500);
        description.push_str("...");
    }

    (title, url, description)
}

/// 解析 `Title:` / `URL:` 字段行（后出现者覆盖先出现者）。
fn parse_text_metadata(block: &str) -> (String, String) {
    let mut title = String::new();
    let mut url = String::new();

    for line in block.lines() {
        let line = line.trim();
        if let Some(t) = line.strip_prefix("Title: ") {
            title = t.trim().to_string();
        } else if let Some(u) = line.strip_prefix("URL: ") {
            url = u.trim().to_string();
        }
    }

    (title, url)
}

/// 描述片段行判定：排除元数据行、空行与省略号行。
fn is_description_line(line: &str) -> bool {
    !line.starts_with("Title: ")
        && !line.starts_with("URL: ")
        && !line.starts_with("Highlights:")
        && !line.starts_with("Published:")
        && !line.starts_with("Author:")
        && !line.is_empty()
        && !line.starts_with("...")
}

/// 拼接描述片段（单空格分隔）。
fn collect_description(block: &str) -> String {
    let mut description = String::new();

    for line in block.lines() {
        let line = line.trim();
        if !is_description_line(line) {
            continue;
        }
        if !description.is_empty() {
            description.push(' ');
        }
        description.push_str(line);
    }

    description
}
