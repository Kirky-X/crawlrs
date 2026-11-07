// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 单链接过滤判定（从 `crawl_link_extractor.rs` 拆分）

use crate::workers::crawl::{DomainFilter, FilterContext, UrlFilter, UrlPatternFilter};

/// 单链接过滤判定（自 [`collect_unique_links`] 提取），返回 `true` 表示
/// 可入集。判定顺序与短路行为逐字保留：http 前缀 → 与源 URL 相同 →
/// 黑名单 → include/exclude patterns → include 为空集合时整体短路。
pub(super) fn url_accepted(
    url_str: &str,
    task_url: &str,
    pattern_filter: &Option<UrlPatternFilter>,
    empty_include_short_circuit: bool,
    blacklist_filter: &Option<DomainFilter>,
    filter_ctx: &FilterContext,
) -> bool {
    if !url_str.starts_with("http") {
        return false;
    }
    if url_str == task_url {
        return false;
    }

    // 黑名单过滤：绝对拒绝，不受 include patterns 影响
    if let Some(bf) = blacklist_filter {
        if !bf.accept(url_str, filter_ctx) {
            return false;
        }
    }

    if let Some(filter) = pattern_filter {
        if !filter.accept(url_str, filter_ctx) {
            return false;
        }
    } else if empty_include_short_circuit {
        return false;
    }

    true
}
