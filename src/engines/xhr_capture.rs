// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! XHR/Fetch 后台响应捕获纯逻辑（吸收点 F6）
//!
//! [`crate::engines::client::playwright`]（`engine-playwright` 门控）在 CDP
//! Network domain 上监听 `EventResponseReceived`，把命中的 XHR/Fetch 响应记入
//! [`XhrCaptureBuffer`]；页面等待策略完成后逐条 `Network.getResponseBody` 取体，
//! 经 [`process_body`] 解码 + 截断后随响应返回。
//!
//! 本模块只放无 CDP 依赖的判定/缓冲/解码纯逻辑：
//! - 不依赖 `chromiumoxide`，使单元测试可在 default 特性集运行；
//! - 监听侧类型过滤（`ResourceType::Xhr | Fetch`）依赖 CDP 枚举，留在 playwright.rs。

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

use crate::engines::types::InternalCapturedResponse;

/// XHR/Fetch 捕获配置（bootstrap 从 `PlaywrightEngineSettings` 映射注入）
#[derive(Debug, Clone)]
pub struct XhrCaptureConfig {
    /// 捕获 URL glob 白名单；空列表 = 捕获全部 XHR/Fetch
    pub url_patterns: Vec<String>,
    /// 单响应体上限（字节），超出丢弃
    pub max_bytes: u64,
    /// 单页面最大捕获响应数，超出停收
    pub max_responses: usize,
}

/// 捕获过程统计（显性化丢弃原因，供日志与测试断言）
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct XhrCaptureStats {
    /// 达 max_responses 上限后被跳过的响应数
    pub overflow_skipped: usize,
    /// 超 max_bytes 被丢弃的响应体数
    pub oversized_dropped: usize,
    /// base64 解码失败数
    pub decode_failed: usize,
    /// `Network.getResponseBody` CDP 调用失败数（body 已被浏览器丢弃等）
    pub fetch_failed: usize,
}

/// 待取响应条目（监听侧记录，等待完成后取体）
#[derive(Debug, Clone)]
pub struct PendingXhr {
    /// CDP `Network.requestId`（`RequestId` 为 String newtype，存原始串，
    /// 取体时经 `GetResponseBodyParams::new(impl Into<RequestId>)` 还原）
    pub request_id: String,
    pub url: String,
    pub status: u16,
    pub mime_type: String,
}

/// 单页面捕获缓冲（监听侧经 Mutex 共享写入）
///
/// 数据流：`ResponseReceived` → [`XhrCaptureBuffer::record`]（cap 判定，等待取体）；
/// `LoadingFinished` → [`XhrCaptureBuffer::take_pending`] 命中后由调用方立即取体
/// （Network domain 存活期内），经 [`process_body`] 后
/// [`XhrCaptureBuffer::push_captured`] 入完成区。
#[derive(Debug)]
pub struct XhrCaptureBuffer {
    max_responses: usize,
    /// 已记录、等待 LoadingFinished 取体
    pending: Vec<PendingXhr>,
    /// 取体完成，待 collect 排空
    captured: Vec<InternalCapturedResponse>,
    /// LoadingFinished 先于 record 到达的 request_id（上限防泄漏）
    finished_early: std::collections::HashSet<String>,
    overflow_skipped: usize,
    oversized_dropped: usize,
    decode_failed: usize,
    fetch_failed: usize,
}

/// [`XhrCaptureBuffer::record`] 结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOutcome {
    /// 已记录，等待 LoadingFinished 后取体
    Recorded,
    /// LoadingFinished 先于本条到达，调用方应立即取体
    AlreadyFinished,
    /// 达 max_responses 上限被跳过（已计数）
    Skipped,
}

impl XhrCaptureBuffer {
    pub fn new(max_responses: usize) -> Self {
        Self {
            max_responses,
            pending: Vec::new(),
            captured: Vec::new(),
            finished_early: std::collections::HashSet::new(),
            overflow_skipped: 0,
            oversized_dropped: 0,
            decode_failed: 0,
            fetch_failed: 0,
        }
    }

    /// 记录一条待取响应；达上限后停收并计数。
    pub fn record(&mut self, entry: PendingXhr) -> RecordOutcome {
        if self.finished_early.remove(&entry.request_id) {
            return RecordOutcome::AlreadyFinished;
        }
        if self.pending.len() + self.captured.len() >= self.max_responses {
            self.overflow_skipped += 1;
            return RecordOutcome::Skipped;
        }
        self.pending.push(entry);
        RecordOutcome::Recorded
    }

    /// 取出该 request_id 的待取条目（LoadingFinished 时调用）；不存在返回 None
    pub fn take_pending(&mut self, request_id: &str) -> Option<PendingXhr> {
        let pos = self
            .pending
            .iter()
            .position(|e| e.request_id == request_id)?;
        Some(self.pending.remove(pos))
    }

    /// 标记 LoadingFinished 先到（record 时转为 [`RecordOutcome::AlreadyFinished`]）
    ///
    /// 集合有上限（max_responses × 4，下限 16），超出丢弃——只影响极端场景
    /// 下早到事件与 record 乱序配对，不产生错误行为。
    pub fn mark_finished_early(&mut self, request_id: &str) {
        let cap = self.max_responses.saturating_mul(4).max(16);
        if self.finished_early.len() < cap {
            self.finished_early.insert(request_id.to_string());
        }
    }

    /// 取体完成入完成区
    pub fn push_captured(&mut self, response: InternalCapturedResponse) {
        self.captured.push(response);
    }

    pub fn note_oversize(&mut self) {
        self.oversized_dropped += 1;
    }

    pub fn note_decode_failed(&mut self) {
        self.decode_failed += 1;
    }

    pub fn note_fetch_failed(&mut self) {
        self.fetch_failed += 1;
    }

    /// 取出全部已完成捕获并清空
    pub fn drain_captured(&mut self) -> Vec<InternalCapturedResponse> {
        std::mem::take(&mut self.captured)
    }

    /// 仍在等待取体的条目数（collect 时未完成即丢弃，供日志显性化）
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// 当前统计
    pub fn stats(&self) -> XhrCaptureStats {
        XhrCaptureStats {
            overflow_skipped: self.overflow_skipped,
            oversized_dropped: self.oversized_dropped,
            decode_failed: self.decode_failed,
            fetch_failed: self.fetch_failed,
        }
    }
}

/// 响应体处理结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyOutcome {
    /// 保留（base64 已解码；二进制按 UTF-8 lossy 转换）
    Kept(String),
    /// 超 max_bytes 丢弃
    Oversize,
    /// base64 解码失败
    DecodeFailed,
}

/// 处理 `Network.getResponseBody` 返回体：base64 解码 + max_bytes 截断判定
///
/// 字节数按解码后计（base64 时），与配置语义（响应体大小）一致。
pub fn process_body(raw: String, base64_encoded: bool, max_bytes: u64) -> BodyOutcome {
    if base64_encoded {
        match BASE64.decode(raw.as_bytes()) {
            Ok(bytes) => {
                if bytes.len() as u64 > max_bytes {
                    return BodyOutcome::Oversize;
                }
                BodyOutcome::Kept(String::from_utf8_lossy(&bytes).into_owned())
            }
            Err(_) => BodyOutcome::DecodeFailed,
        }
    } else if raw.len() as u64 > max_bytes {
        BodyOutcome::Oversize
    } else {
        BodyOutcome::Kept(raw)
    }
}

/// URL glob 匹配：支持 `*`（任意序列，含 `/`——URL 非路径感知）与 `?`（单字符）
///
/// 复用 [`crate::utils::glob::glob_match`] 单实现（迭代双指针，
/// 最坏 O(len(pattern) × len(text))，无递归回溯指数放大）。
pub use crate::utils::glob::glob_match;

/// 捕获 URL 过滤：patterns 为空 = 全收；否则任一 pattern 命中即捕获
pub fn url_matches_capture_patterns(url: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return true;
    }
    patterns.iter().any(|p| glob_match(p, url))
}

#[cfg(test)]
mod tests {
    use super::*;

    // === glob_match ===

    #[test]
    fn glob_match_star_spans_path_separators() {
        // URL glob：* 可跨 /，"*/api/*" 语义为"URL 中含 /api/ 段"
        assert!(glob_match("*/api/*", "https://example.com/api/data"));
        assert!(glob_match(
            "*/api/*",
            "https://example.com/api/v2/users?id=1"
        ));
    }

    #[test]
    fn glob_match_rejects_non_matching() {
        assert!(!glob_match("*/api/*", "https://example.com/static/app.js"));
        assert!(!glob_match("*/api/*", "https://api.example.com/v2/other"));
    }

    #[test]
    fn glob_match_exact_and_wildcard_forms() {
        assert!(glob_match(
            "https://api.example.com/*",
            "https://api.example.com/v1/users"
        ));
        assert!(glob_match("*", "anything at all"));
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "other"));
        assert!(glob_match("*api*", "https://x.com/api"));
    }

    #[test]
    fn glob_match_question_mark_requires_exactly_one_char() {
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(!glob_match("a?c", "abbc"));
    }

    #[test]
    fn glob_match_empty_pattern_only_matches_empty() {
        assert!(glob_match("", ""));
        assert!(!glob_match("", "x"));
    }

    #[test]
    fn glob_match_leading_and_trailing_star() {
        assert!(glob_match("*/data", "https://x.com/a/b/data"));
        assert!(glob_match("https://x.com/*", "https://x.com/"));
        assert!(!glob_match("https://x.com/*", "https://x.com"));
    }

    // === url_matches_capture_patterns ===

    #[test]
    fn empty_patterns_capture_all() {
        assert!(url_matches_capture_patterns(
            "https://example.com/anything",
            &[]
        ));
    }

    #[test]
    fn single_pattern_filters() {
        let patterns = vec!["*/api/*".to_string()];
        assert!(url_matches_capture_patterns(
            "https://example.com/api/data",
            &patterns
        ));
        assert!(!url_matches_capture_patterns(
            "https://example.com/static/app.js",
            &patterns
        ));
    }

    #[test]
    fn multiple_patterns_any_match() {
        let patterns = vec!["*/a/*".to_string(), "*/b/*".to_string()];
        assert!(url_matches_capture_patterns(
            "https://x.com/b/item",
            &patterns
        ));
        assert!(!url_matches_capture_patterns(
            "https://x.com/c/item",
            &patterns
        ));
    }

    #[test]
    fn regex_like_metachars_are_literal_no_panic() {
        // 只支持 * ?，其余 glob 元字符按字面量处理，不得 panic
        let patterns = vec!["*[id=1]*".to_string()];
        assert!(!url_matches_capture_patterns(
            "https://x.com/item",
            &patterns
        ));
        assert!(url_matches_capture_patterns(
            "https://x.com/[id=1]",
            &patterns
        ));
    }

    // === XhrCaptureBuffer ===

    fn pending(id: &str) -> PendingXhr {
        PendingXhr {
            request_id: id.to_string(),
            url: format!("https://example.com/api/{}", id),
            status: 200,
            mime_type: "application/json".to_string(),
        }
    }

    fn captured(id: &str) -> InternalCapturedResponse {
        InternalCapturedResponse {
            url: format!("https://example.com/api/{}", id),
            status: 200,
            mime_type: "application/json".to_string(),
            body: format!("body-{}", id),
        }
    }

    #[test]
    fn buffer_records_and_takes_pending() {
        let mut buf = XhrCaptureBuffer::new(3);
        assert_eq!(buf.record(pending("1")), RecordOutcome::Recorded);
        assert_eq!(buf.record(pending("2")), RecordOutcome::Recorded);
        assert_eq!(buf.stats().overflow_skipped, 0);

        let entry = buf.take_pending("1").expect("pending entry must exist");
        assert_eq!(entry.url, "https://example.com/api/1");
        assert_eq!(entry.status, 200);
        assert_eq!(entry.mime_type, "application/json");
        // 取走即移除
        assert!(buf.take_pending("1").is_none());
        assert!(buf.take_pending("missing").is_none());
    }

    #[test]
    fn buffer_stops_at_max_responses_and_counts() {
        let mut buf = XhrCaptureBuffer::new(2);
        assert_eq!(buf.record(pending("1")), RecordOutcome::Recorded);
        assert_eq!(buf.record(pending("2")), RecordOutcome::Recorded);
        // 第 N+1 条起不捕获
        assert_eq!(buf.record(pending("3")), RecordOutcome::Skipped);
        assert_eq!(buf.record(pending("4")), RecordOutcome::Skipped);
        assert_eq!(buf.stats().overflow_skipped, 2);
        assert_eq!(buf.pending_len(), 2);
    }

    #[test]
    fn buffer_counts_captured_toward_cap() {
        let mut buf = XhrCaptureBuffer::new(2);
        assert_eq!(buf.record(pending("1")), RecordOutcome::Recorded);
        let entry = buf.take_pending("1").unwrap();
        buf.push_captured(captured(&entry.request_id));
        assert_eq!(buf.record(pending("2")), RecordOutcome::Recorded);
        let entry = buf.take_pending("2").unwrap();
        buf.push_captured(captured(&entry.request_id));
        // 完成捕获同样占用容量（防内存放大）
        assert_eq!(buf.record(pending("3")), RecordOutcome::Skipped);
        assert_eq!(buf.stats().overflow_skipped, 1);
    }

    #[test]
    fn buffer_zero_cap_skips_everything() {
        let mut buf = XhrCaptureBuffer::new(0);
        assert_eq!(buf.record(pending("1")), RecordOutcome::Skipped);
        assert_eq!(buf.stats().overflow_skipped, 1);
        assert!(buf.drain_captured().is_empty());
    }

    #[test]
    fn buffer_early_finish_flow_yields_already_finished() {
        // LoadingFinished 先于 record 到达（跨事件流乱序）
        let mut buf = XhrCaptureBuffer::new(3);
        assert!(buf.take_pending("9").is_none());
        buf.mark_finished_early("9");
        assert_eq!(buf.record(pending("9")), RecordOutcome::AlreadyFinished);
        // AlreadyFinished 不占用 pending/容量
        assert_eq!(buf.pending_len(), 0);
        // 未配对的早到标记不改变行为
        buf.mark_finished_early("ghost");
        assert_eq!(buf.record(pending("1")), RecordOutcome::Recorded);
    }

    #[test]
    fn buffer_finished_early_set_is_bounded() {
        let mut buf = XhrCaptureBuffer::new(4);
        let cap = 4 * 4;
        for i in 0..(cap + 10) {
            buf.mark_finished_early(&format!("id-{i}"));
        }
        // 超上限后新标记被丢弃：不产生 AlreadyFinished
        assert_eq!(buf.record(pending("untracked")), RecordOutcome::Recorded);
    }

    #[test]
    fn buffer_drain_captured_round_trip() {
        let mut buf = XhrCaptureBuffer::new(4);
        buf.push_captured(captured("a"));
        buf.push_captured(captured("b"));
        let drained = buf.drain_captured();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].url, "https://example.com/api/a");
        assert_eq!(drained[0].body, "body-a");
        assert_eq!(drained[1].body, "body-b");
        assert!(buf.drain_captured().is_empty());
    }

    #[test]
    fn buffer_stats_reflect_drop_reasons() {
        let mut buf = XhrCaptureBuffer::new(4);
        buf.note_oversize();
        buf.note_decode_failed();
        buf.note_fetch_failed();
        let stats = buf.stats();
        assert_eq!(stats.oversized_dropped, 1);
        assert_eq!(stats.decode_failed, 1);
        assert_eq!(stats.fetch_failed, 1);
        assert_eq!(stats.overflow_skipped, 0);
    }

    // === process_body ===

    #[test]
    fn process_body_plain_text_under_limit_kept() {
        let out = process_body("hello".to_string(), false, 1024);
        assert_eq!(out, BodyOutcome::Kept("hello".to_string()));
    }

    #[test]
    fn process_body_plain_text_over_limit_dropped() {
        let big = "x".repeat(4096);
        let out = process_body(big.clone(), false, 256);
        assert_eq!(out, BodyOutcome::Oversize);
        // 等于上限不丢弃（"超出截断丢弃"为严格大于）
        assert_eq!(
            process_body("x".repeat(256), false, 256),
            BodyOutcome::Kept("x".repeat(256))
        );
        let _ = big;
    }

    #[test]
    fn process_body_base64_decoded() {
        use base64::Engine as _;
        let encoded = BASE64.encode("{\"ok\":true}");
        let out = process_body(encoded, true, 1024);
        assert_eq!(out, BodyOutcome::Kept("{\"ok\":true}".to_string()));
    }

    #[test]
    fn process_body_base64_oversize_measured_after_decode() {
        use base64::Engine as _;
        // 解码后 2048 字节 > max_bytes 1024：按解码后大小判
        let encoded = BASE64.encode(vec![b'a'; 2048]);
        assert_eq!(process_body(encoded, true, 1024), BodyOutcome::Oversize);
        // 同样内容放宽上限后保留
        let encoded2 = BASE64.encode(vec![b'a'; 2048]);
        assert!(matches!(
            process_body(encoded2, true, 4096),
            BodyOutcome::Kept(_)
        ));
    }

    #[test]
    fn process_body_base64_invalid_fails_explicitly() {
        assert_eq!(
            process_body("!!!not base64!!!".to_string(), true, 1024),
            BodyOutcome::DecodeFailed
        );
    }

    #[test]
    fn process_body_empty_body_kept() {
        assert_eq!(
            process_body(String::new(), false, 1024),
            BodyOutcome::Kept(String::new())
        );
    }
}
