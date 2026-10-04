// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 软 404 指纹检测（吸收点 F11，参考 crawl4ai `Soft404Fingerprint`）
//!
//! 空壳页（SPA 回退壳、模板占位页）常以 200 返回与真实 404 页相同的内容。
//! 检测器从真实 404/410 响应学习空壳指纹
//! `{status, title_hash, content_length, body_hash(sha2-256[:16])}`，
//! 后续同指纹的 200 响应判为软 404：不落结果、计软 404 计数；
//! 任务照常 mark_completed（防重试风暴）。
//!
//! # 匹配语义
//!
//! 学习/命中的匹配键是**内容三元组** `(title_hash, content_length, body_hash)`，
//! `status_code` 仅作为诊断信息随指纹记录——否则 200 壳永远无法命中
//! 404 学到的指纹（测试 [`tests::learn_from_404_then_200_hits`] 固化此语义）。
//!
//! # 生命周期
//!
//! 进程内（对齐 [`crate::utils::dedup::Deduplicator`] 语义），重启重学习
//! 可接受——404 页会再次出现。指纹库容量达 [`Soft404Detector::max_fingerprints`]
//! 后按插入序淘汰最旧（FIFO），旧指纹淘汰后不再命中。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use dashmap::DashMap;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};

/// 软 404 指纹（crawl4ai Soft404Fingerprint 四元组语义）
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Soft404Fingerprint {
    /// 产生指纹的响应状态码（诊断信息，不参与匹配，见模块文档）
    pub status_code: u16,
    /// `<title>` 文本的哈希（sha2-256 前 8 字节）
    pub title_hash: u64,
    /// 响应体字节数
    pub content_length: usize,
    /// 响应体哈希（sha2-256 前 16 字节）
    pub body_hash: [u8; 16],
}

/// 学习/命中匹配键：内容三元组（不含 status_code）
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ContentKey {
    title_hash: u64,
    content_length: usize,
    body_hash: [u8; 16],
}

impl Soft404Fingerprint {
    /// 从响应构造指纹（`content` 为处理后的响应体文本）
    pub fn of(status_code: u16, content: &str) -> Self {
        let title = extract_html_title(content);
        Self {
            status_code,
            title_hash: sha256_prefix8(title.as_bytes()),
            content_length: content.len(),
            body_hash: sha256_prefix16(content.as_bytes()),
        }
    }

    fn content_key(&self) -> ContentKey {
        ContentKey {
            title_hash: self.title_hash,
            content_length: self.content_length,
            body_hash: self.body_hash,
        }
    }
}

/// 一条已学习指纹的元数据
#[derive(Debug)]
struct Learning {
    /// 学习来源的响应状态码（404 / 410）
    status_code: u16,
    /// 学习时刻（诊断）
    learned_at: std::time::Instant,
    /// 该指纹被判软 404 的次数
    hits: u64,
}

/// 响应级软 404 判定结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Soft404Verdict {
    /// 404/410 响应：指纹已学习（响应本身按既有路径处理）
    Learned,
    /// 200 响应命中已学空壳：调用方跳过结果落库
    Soft404,
    /// 正常响应：照常落库
    Keep,
}

/// 软 404 指纹检测器
///
/// 线程安全（`DashMap` + `Mutex` 队列），所有 worker 共享同一实例
/// （全局单例经 [`shared`] 访问）。
pub struct Soft404Detector {
    /// 已学习指纹（键为内容三元组，见模块文档）
    learned: DashMap<ContentKey, Learning>,
    /// 插入序队列（FIFO 淘汰用；可含已被淘汰的陈旧键，弹出时跳过）
    insertion_order: parking_lot::Mutex<VecDeque<ContentKey>>,
    /// 指纹库容量上限
    max_fingerprints: usize,
    /// 累计软 404 命中次数
    hit_count: AtomicU64,
}

/// `shared()` 惰性初始化时的默认容量（与 `config/default.toml`
/// `crawl.soft404_max_fingerprints` 默认值一致）
pub const DEFAULT_MAX_FINGERPRINTS: usize = 10_000;

/// 全局单例（crawl 路径按进程共享；bootstrap 期经 [`init_shared`]
/// 按 `crawl.soft404_max_fingerprints` 配置初始化）
static SHARED_DETECTOR: OnceLock<Arc<Soft404Detector>> = OnceLock::new();

/// 获取进程级共享检测器（未显式初始化时以 [`DEFAULT_MAX_FINGERPRINTS`] 惰性构造）
pub fn shared() -> Arc<Soft404Detector> {
    SHARED_DETECTOR
        .get_or_init(|| Arc::new(Soft404Detector::new(DEFAULT_MAX_FINGERPRINTS)))
        .clone()
}

/// 按 `crawl.soft404_max_fingerprints` 配置初始化全局检测器。
///
/// 已初始化时保留现例并 `warn!`（容量变更需重启生效）。
pub fn init_shared(max_fingerprints: usize) {
    if SHARED_DETECTOR
        .set(Arc::new(Soft404Detector::new(max_fingerprints)))
        .is_err()
    {
        log::warn!(
            "soft404 detector already initialized; ignoring soft404_max_fingerprints={max_fingerprints} (restart required to apply)"
        );
    }
}

impl Soft404Detector {
    /// 构造检测器（`max_fingerprints` 为指纹库容量上限）
    pub fn new(max_fingerprints: usize) -> Self {
        Self {
            learned: DashMap::new(),
            insertion_order: Mutex::new(VecDeque::new()),
            max_fingerprints: max_fingerprints.max(1),
            hit_count: AtomicU64::new(0),
        }
    }

    /// 当前指纹库条目数
    pub fn len(&self) -> usize {
        self.learned.len()
    }

    /// 指纹库是否为空
    pub fn is_empty(&self) -> bool {
        self.learned.is_empty()
    }

    /// 累计软 404 命中次数
    pub fn hit_count(&self) -> u64 {
        self.hit_count.load(Ordering::Relaxed)
    }

    /// 学习一条指纹（404/410 响应调用）
    ///
    /// 已存在的指纹不重复入队（不刷新淘汰序）；库满时按插入序淘汰最旧。
    pub fn learn(&self, fingerprint: Soft404Fingerprint) {
        let key = fingerprint.content_key();
        if let Some(mut entry) = self.learned.get_mut(&key) {
            entry.status_code = fingerprint.status_code;
            entry.learned_at = std::time::Instant::now();
            return;
        }
        // 容量护栏：满员先淘汰最旧（跳过队列中已失效的陈旧键）
        while self.learned.len() >= self.max_fingerprints {
            let evicted = self
                .insertion_order
                .lock()
                .pop_front()
                .and_then(|stale| self.learned.remove(&stale).map(|_| stale));
            if evicted.is_none() {
                break;
            }
        }
        self.insertion_order.lock().push_back(key.clone());
        self.learned.insert(
            key,
            Learning {
                status_code: fingerprint.status_code,
                learned_at: std::time::Instant::now(),
                hits: 0,
            },
        );
    }

    /// 判定 200 响应是否命中已学空壳
    ///
    /// 命中时累计计数并 `log::info`（含指纹学习来源与命中次数）。
    /// 空库快速路径：未学到任何指纹前不做内容哈希。
    pub fn is_soft404(&self, fingerprint: &Soft404Fingerprint) -> bool {
        if self.learned.is_empty() {
            return false;
        }
        let key = fingerprint.content_key();
        let Some(mut learning) = self.learned.get_mut(&key) else {
            return false;
        };
        let hits = learning.hits + 1;
        learning.hits = hits;
        let learned_from = learning.status_code;
        let total = self.hit_count.fetch_add(1, Ordering::Relaxed) + 1;
        log::info!(
            "soft 404 hit: status {} response matches shell learned from HTTP {} \
             (content_length={}, body_hash={:02x?}, fingerprint hits={hits}, total hits={total})",
            fingerprint.status_code,
            learned_from,
            fingerprint.content_length,
            fingerprint.body_hash,
        );
        true
    }

    /// 响应级判定入口：404/410 学习，200 命中检测，其余 Keep
    pub fn observe(&self, status_code: u16, content: &str) -> Soft404Verdict {
        match status_code {
            404 | 410 => {
                self.learn(Soft404Fingerprint::of(status_code, content));
                Soft404Verdict::Learned
            }
            200 => {
                // 空库快速路径前移：未学到任何指纹时命中必为 false，
                // 不必为每页付出指纹构造开销（64KB 探测 + 全文 SHA-256）
                if self.learned.is_empty() {
                    return Soft404Verdict::Keep;
                }
                let fingerprint = Soft404Fingerprint::of(status_code, content);
                if self.is_soft404(&fingerprint) {
                    Soft404Verdict::Soft404
                } else {
                    Soft404Verdict::Keep
                }
            }
            _ => Soft404Verdict::Keep,
        }
    }
}

/// crawl 成功路径的判定入口（`crawl_task::handle_crawl_success` 调用）
///
/// `enabled = false`（`crawl.soft404_enabled`）时恒 [`Soft404Verdict::Keep`]
/// ——不学习、不命中，200 壳照常落库（回归语义）。
pub fn observe_crawl_response(enabled: bool, status_code: u16, content: &str) -> Soft404Verdict {
    if !enabled {
        return Soft404Verdict::Keep;
    }
    shared().observe(status_code, content)
}

/// 从 HTML 提取 `<title>` 文本（大小写不敏感；仅在响应体前 64KB 内探测，
/// 未找到返回空串——title 缺失时指纹退化为 `(0, len, body_hash)`）
fn extract_html_title(html: &str) -> &str {
    const HTML_PROBE_BYTES: usize = 64 * 1024;
    let probe = if html.len() <= HTML_PROBE_BYTES {
        html
    } else {
        let mut end = HTML_PROBE_BYTES;
        while end > 0 && !html.is_char_boundary(end) {
            end -= 1;
        }
        &html[..end]
    };
    let lower = probe.to_ascii_lowercase();
    let start_tag = match lower.find("<title") {
        Some(i) => i,
        None => return "",
    };
    let content_start = match lower[start_tag..].find('>') {
        Some(rel) => start_tag + rel + 1,
        None => return "",
    };
    let end_rel = match lower[content_start..].find("</title") {
        Some(e) => e,
        None => return "",
    };
    probe[content_start..content_start + end_rel].trim()
}

fn sha256_prefix8(data: &[u8]) -> u64 {
    let digest = Sha256::digest(data);
    u64::from_be_bytes(digest[..8].try_into().expect("sha256 digest >= 8 bytes"))
}

fn sha256_prefix16(data: &[u8]) -> [u8; 16] {
    let digest = Sha256::digest(data);
    digest[..16].try_into().expect("sha256 digest >= 16 bytes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(title: &str, body: &str) -> String {
        format!("<html><head><title>{title}</title></head><body>{body}</body></html>")
    }

    // ========== 指纹构造 ==========

    /// 同内容同状态 → 指纹相同（确定性）；status/内容任一不同 → 不同
    #[test]
    fn fingerprint_is_deterministic_and_discriminating() {
        let a = Soft404Fingerprint::of(404, &page("Not Found", "nothing here"));
        let b = Soft404Fingerprint::of(404, &page("Not Found", "nothing here"));
        assert_eq!(a, b, "同内容指纹必须一致");

        let diff_body = Soft404Fingerprint::of(404, &page("Not Found", "different"));
        assert_ne!(a.body_hash, diff_body.body_hash);
        assert_ne!(a.content_length, diff_body.content_length);

        let diff_title = Soft404Fingerprint::of(404, &page("Other", "nothing here"));
        assert_ne!(a.title_hash, diff_title.title_hash);
    }

    /// `<title>` 提取：大小写不敏感、trim、缺 title 返回空串、
    /// 超长 body 不 panic（多字节字符在探测边界处截断安全）
    #[test]
    fn extract_html_title_semantics() {
        assert_eq!(
            extract_html_title("<TITLE>  Big Case  </TITLE>"),
            "Big Case"
        );
        assert_eq!(extract_html_title("<title attr=\"x\">v</title>"), "v");
        assert_eq!(extract_html_title("<html><body>no title</body></html>"), "");
        // UTF-8 多字节字符跨越 64KB 探测边界：截断须落在字符边界
        let filler = "字".repeat(40_000);
        let html = format!("<html>{filler}<title>t</title></html>");
        let _ = extract_html_title(&html);
    }

    // ========== 学习 / 命中 ==========

    /// 核心语义：先 404 学习，再同内容 200 命中（status 不参与匹配）
    #[test]
    fn learn_from_404_then_200_hits() {
        let detector = Soft404Detector::new(100);
        let shell = page("Not Found", "empty shell");

        assert_eq!(
            detector.observe(404, &shell),
            Soft404Verdict::Learned,
            "404 响应触发学习"
        );
        assert_eq!(detector.len(), 1);

        assert_eq!(
            detector.observe(200, &shell),
            Soft404Verdict::Soft404,
            "同指纹的 200 壳必须命中"
        );
        assert_eq!(detector.hit_count(), 1);
    }

    /// 410 同样触发学习
    #[test]
    fn learn_from_410() {
        let detector = Soft404Detector::new(100);
        assert_eq!(
            detector.observe(410, &page("Gone", "gone forever")),
            Soft404Verdict::Learned
        );
    }

    /// 未学习过的 200 → Keep（正常落库）
    #[test]
    fn unlearned_200_keeps() {
        let detector = Soft404Detector::new(100);
        let shell = page("Not Found", "empty shell");
        assert_eq!(detector.observe(200, &shell), Soft404Verdict::Keep);
        assert_eq!(detector.hit_count(), 0);
    }

    /// 内容任一分量不同 → 不命中
    #[test]
    fn different_content_does_not_hit() {
        let detector = Soft404Detector::new(100);
        detector.observe(404, &page("Not Found", "shell v1"));
        // 同 title 同长度但 body 不同
        let probe = Soft404Fingerprint::of(200, &page("Not Found", "shell v2"));
        assert!(!detector.is_soft404(&probe));
    }

    /// 空库快速路径：未学习前 200 直接 Keep（不做哈希）
    #[test]
    fn empty_library_fast_path_keeps() {
        let detector = Soft404Detector::new(100);
        assert!(detector.is_empty());
        assert_eq!(
            detector.observe(200, &page("Not Found", "anything")),
            Soft404Verdict::Keep
        );
    }

    // ========== 容量淘汰 ==========

    /// 库满后 FIFO 淘汰最旧指纹：旧指纹不再命中，新指纹可命中
    #[test]
    fn max_fingerprints_evicts_oldest() {
        let detector = Soft404Detector::new(2);
        let first = page("T1", "body-1");
        let second = page("T2", "body-2");
        let third = page("T3", "body-3");
        detector.observe(404, &first);
        detector.observe(404, &second);
        assert_eq!(detector.len(), 2);

        // 第三条挤掉第一条
        detector.observe(404, &third);
        assert_eq!(detector.len(), 2, "容量上限 2");

        let first_fp = Soft404Fingerprint::of(200, &first);
        assert!(
            !detector.is_soft404(&first_fp),
            "被淘汰的最旧指纹不得再命中"
        );
        let third_fp = Soft404Fingerprint::of(200, &third);
        assert!(detector.is_soft404(&third_fp), "新指纹仍可命中");
    }

    /// 重复学习同一指纹不重复入队（不挤占容量、不刷新淘汰序）
    #[test]
    fn relearn_same_fingerprint_keeps_single_entry() {
        let detector = Soft404Detector::new(2);
        let shell = page("T", "same");
        detector.observe(404, &shell);
        detector.observe(404, &shell);
        detector.observe(404, &shell);
        assert_eq!(detector.len(), 1, "同指纹重复学习只保留一条");
    }

    // ========== observe 其余分支 / 全局开关 ==========

    /// 非 404/410/200 的状态码 → Keep 且不学习
    #[test]
    fn other_status_codes_keep() {
        let detector = Soft404Detector::new(100);
        assert_eq!(
            detector.observe(500, &page("Err", "boom")),
            Soft404Verdict::Keep
        );
        assert_eq!(
            detector.observe(301, &page("Move", "x")),
            Soft404Verdict::Keep
        );
        assert!(detector.is_empty(), "非学习状态码不得入库");
    }

    /// enabled=false（crawl.soft404_enabled=false）时 observe_crawl_response
    /// 恒 Keep——不学习不命中（回归：200 壳照常落库）
    #[test]
    fn observe_crawl_response_disabled_keeps() {
        let shell = page("Not Found", "shell");
        assert_eq!(
            observe_crawl_response(false, 404, &shell),
            Soft404Verdict::Keep,
            "关闭时 404 也不学习"
        );
        assert_eq!(
            observe_crawl_response(false, 200, &shell),
            Soft404Verdict::Keep,
            "关闭时 200 壳照常落库"
        );
    }

    /// max_fingerprints 下限护栏：0 视为 1（不允许零容量死库）
    #[test]
    fn zero_capacity_clamps_to_one() {
        let detector = Soft404Detector::new(0);
        detector.observe(404, &page("A", "a"));
        assert_eq!(detector.len(), 1, "容量下限为 1");
    }
}
