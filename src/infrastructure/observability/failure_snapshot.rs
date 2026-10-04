// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 失败现场快照（吸收点 F4，参考 Crawlee `ErrorSnapshotter`）
//!
//! 抓取被判定为反爬/限速时，把响应现场（HTML + 响应元数据 + 可选截图）
//! 按错误签名键去重存档，供事后诊断 WAF 拦截页形态。
//!
//! 设计约束：
//! - **旁路观测**：内部任何 IO 失败 `log::debug` 后静默返回，绝不影响抓取主流程
//!   （调用方无需 `?`）；快照器未配置（`Option::None`）时调用点零成本短路；
//!   全部落盘经 `spawn_blocking` 执行，不阻塞 tokio worker 线程
//! - **签名去重**：同签名键第二次命中只自增计数并刷新 `.meta.json` 的 count，
//!   不重写 `.html`/`.jpg`（对齐 Crawlee 去重语义）
//! - **磁盘护栏（滚动）**：全目录文件数达 `max_total` 后按最旧优先滚动删除腾位，
//!   目录文件数有界；签名键内存表达 [`MAX_TRACKED_KEYS`] 上限，超限仅计指标
//! - **权限收紧**：快照文件以 0600 落盘（unix；其他平台走进程默认 umask）
//!
//! 落盘形态（`{key}` = sha256(url|status|reason) 前 30 hex + 消毒 reason 前 30 字符）：
//! - `{key}.html`：响应体
//! - `{key}.meta.json`：url / status / reason / count / timestamp / headers
//! - `{key}.jpg`：screenshot 为 `Some` 时 base64 解码写入（仅透传已有值，不额外触发截图）
//!
//! 敏感信息提示：响应体可能含目标页敏感内容，目录访问权限与保留期由运维治理。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 签名键内存表（`counts`）的容量上限：键含 URL，长期被反爬拦截的爬取下
/// 不同 URL 无界增长会泄漏内存；达上限后新键仅计指标，不再入表/落盘
const MAX_TRACKED_KEYS: usize = 10_000;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use dashmap::DashMap;
use sha2::{Digest, Sha256};

use crate::common::metrics_shim::counter;
use crate::config::settings::ObservabilitySettings;

/// 一次失败现场的快照输入（全借用；落盘内容在内部拷贝进阻塞任务）
pub struct SnapshotInput<'a> {
    /// 请求 URL（签名键成分）
    pub url: &'a str,
    /// 响应状态码（签名键成分）
    pub status_code: u16,
    /// 失败原因（签名键成分 + meta.json 字段）
    pub reason: &'a str,
    /// 响应体（`.html` 落盘内容）
    pub content: &'a str,
    /// 响应头（meta.json 字段）
    pub headers: &'a HashMap<String, String>,
    /// 截图 base64（`Some` 时解码写 `.jpg`；仅浏览器引擎已产出时非空）
    pub screenshot: Option<&'a str>,
}

/// 失败现场快照器：错误签名键去重存档
///
/// - `snapshot`：同键首次落盘 `.html` + `.meta.json`（+ 可选 `.jpg`），
///   后续命中只自增计数并刷新 meta 的 count
/// - 内部全 `DashMap` + `AtomicU64`，无锁线程安全；`snapshot(&self)` 可并发调用
/// - 磁盘按 `max_total` 滚动有界；签名键内存表按 [`MAX_TRACKED_KEYS`] 封顶
#[derive(Debug)]
pub struct FailureSnapshotter {
    /// 快照落盘目录
    dir: PathBuf,
    /// 签名键 → 命中次数（去重计数）
    counts: DashMap<String, u64>,
    /// 全目录已写文件数（磁盘护栏游标）
    total_written: AtomicU64,
    /// 全目录快照文件总数上限
    max_total: usize,
}

impl FailureSnapshotter {
    /// 指定目录与文件总数上限创建快照器
    ///
    /// 目录在首次 `snapshot` 时惰性创建；创建失败按旁路观测静默降级。
    pub fn new(dir: impl Into<PathBuf>, max_total: usize) -> Self {
        Self {
            dir: dir.into(),
            counts: DashMap::new(),
            total_written: AtomicU64::new(0),
            max_total: max_total.max(1),
        }
    }

    /// 从 `[observability]` 配置构造
    ///
    /// `failure_snapshot_dir` 为空串 = 禁用（返回 `None`，全链路零成本）。
    pub fn from_config(settings: &ObservabilitySettings) -> Option<Arc<Self>> {
        if settings.failure_snapshot_dir.is_empty() {
            return None;
        }
        Some(Arc::new(Self::new(
            settings.failure_snapshot_dir.clone(),
            settings.failure_snapshot_max_total,
        )))
    }

    /// 记录一次失败现场（旁路观测，永不失败）
    ///
    /// 同签名键第二次命中只自增计数并刷新 `.meta.json` 的 count，
    /// 不重写 `.html`/`.jpg`；目录文件数达 `max_total` 后按最旧优先滚动
    /// 删除腾位（新签名仍可观测）。内部 IO 失败 `log::debug` 后静默返回。
    ///
    /// 全部文件 IO 经 `spawn_blocking` 执行，调用方（async 路由路径）的
    /// tokio worker 线程不被阻塞。
    pub async fn snapshot(&self, input: SnapshotInput<'_>) {
        let key = Self::signature_key(input.url, input.status_code, input.reason);
        // 命中计数先于磁盘判断：内存表达 [`MAX_TRACKED_KEYS`] 上限，
        // 超限后新键仅计指标（含 URL 的键不无限驻留内存）
        let count = match self.counts.get_mut(&key) {
            Some(mut c) => {
                *c += 1;
                *c
            }
            None => {
                if self.counts.len() >= MAX_TRACKED_KEYS {
                    counter!("crawlrs_failure_snapshot_total").increment(1);
                    return;
                }
                self.counts.insert(key.clone(), 1);
                1
            }
        };
        counter!("crawlrs_failure_snapshot_total").increment(1);

        // meta.json 每次命中都刷新（小文件）：count 反映累计命中次数；
        // 刷新同时续期 mtime，滚动删除时活跃键的 meta 不会被优先淘汰
        let meta = serde_json::json!({
            "url": input.url,
            "status": input.status_code,
            "reason": input.reason,
            "count": count,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "headers": input.headers,
        });

        let dir = self.dir.clone();
        let key_closure = key.clone();
        let content = input.content.to_string();
        // base64 解码放调用线程（纯 CPU、输入生命周期受限），落盘留阻塞池
        let screenshot = input
            .screenshot
            .map(|s| BASE64.decode(s))
            .transpose()
            .ok()
            .flatten();
        let max_total = self.max_total;

        let outcome = tokio::task::spawn_blocking(move || {
            persist_snapshot(
                &dir,
                &key_closure,
                count,
                &content,
                screenshot.as_deref(),
                &meta,
                max_total,
            )
        })
        .await;

        match outcome {
            Ok((written, evicted)) => {
                if written > 0 {
                    self.total_written.fetch_add(written, Ordering::Relaxed);
                }
                if evicted > 0 {
                    self.total_written.fetch_sub(
                        evicted.min(self.total_written.load(Ordering::Relaxed)),
                        Ordering::Relaxed,
                    );
                }
            }
            Err(e) => log::debug!("failure snapshot: blocking task join failed: {e}"),
        }
    }

    /// 错误签名键 = sha256(url|status|reason) 前 30 hex + 消毒 reason 前 30 字符
    ///
    /// 消毒仅保留 `[a-zA-Z0-9_-]`（剥路径分隔符/控制字符），保证键可安全作文件名。
    fn signature_key(url: &str, status_code: u16, reason: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        hasher.update(b"|");
        hasher.update(status_code.to_string().as_bytes());
        hasher.update(b"|");
        hasher.update(reason.as_bytes());
        let hex = hex::encode(hasher.finalize());

        let mut key = String::with_capacity(60);
        key.push_str(&hex[..30]);
        key.extend(
            reason
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .take(30),
        );
        key
    }

    /// 查询某签名键的命中次数（测试与监控用）
    pub fn hit_count(&self, key: &str) -> Option<u64> {
        self.counts.get(key).map(|c| *c)
    }

    /// 查询全目录已写文件数（测试与监控用）
    pub fn total_written(&self) -> u64 {
        self.total_written.load(Ordering::Relaxed)
    }
}

/// 落盘一次快照（阻塞上下文调用，供 `spawn_blocking` 承载）
///
/// 返回 `(写入文件数, 滚动删除文件数)`。目录文件数达 `max_total` 时按
/// mtime 最旧优先逐个删除腾位；重复命中（count > 1）只刷新 meta，
/// 不做目录扫描。任何失败 `log::debug` 后静默返回（旁路观测语义）。
fn persist_snapshot(
    dir: &Path,
    key: &str,
    count: u64,
    content: &str,
    screenshot: Option<&[u8]>,
    meta: &serde_json::Value,
    max_total: usize,
) -> (u64, u64) {
    let mut written = 0u64;
    let mut evicted = 0u64;

    if let Err(e) = fs::create_dir_all(dir) {
        log::debug!(
            "failure snapshot: cannot create dir {}: {}",
            dir.display(),
            e
        );
        return (0, 0);
    }

    if count == 1 {
        // 首次命中才落盘响应体与截图（重复命中不重写，对齐 Crawlee 去重语义）；
        // 需要新增文件前先滚动腾位
        let needed: u64 = 1 + u64::from(screenshot.is_some()) + 1; // html + jpg? + meta
        evicted += evict_oldest(dir, needed, max_total);

        let html_path = dir.join(format!("{key}.html"));
        match write_private(&html_path, content.as_bytes()) {
            Ok(()) => written += 1,
            Err(e) => {
                log::debug!(
                    "failure snapshot: write {} failed: {}",
                    html_path.display(),
                    e
                )
            }
        }
        if let Some(bytes) = screenshot {
            let jpg_path = dir.join(format!("{key}.jpg"));
            match write_private(&jpg_path, bytes) {
                Ok(()) => written += 1,
                Err(e) => log::debug!(
                    "failure snapshot: write {} failed: {}",
                    jpg_path.display(),
                    e
                ),
            }
        }
    }

    // meta.json 每次命中都刷新（小文件）：count 反映累计命中次数
    let meta_path = dir.join(format!("{key}.meta.json"));
    match serde_json::to_string_pretty(meta)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        .and_then(|raw| write_private(&meta_path, raw.as_bytes()))
    {
        Ok(()) => written += 1,
        Err(e) => {
            log::debug!(
                "failure snapshot: write {} failed: {}",
                meta_path.display(),
                e
            )
        }
    }
    (written, evicted)
}

/// 目录文件数逼近 `max_total` 时按 mtime 最旧优先删除，为 `needed` 个新文件腾位
///
/// 返回实际删除的文件数。文件级逐个删除（不按 key 分组）：活跃键的
/// meta 因每次命中刷新 mtime，天然不会被优先淘汰。目录读取失败视为
/// 无需腾位（后续写盘失败按旁路静默）。
fn evict_oldest(dir: &Path, needed: u64, max_total: usize) -> u64 {
    let entries = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            log::debug!("failure snapshot: read_dir {} failed: {}", dir.display(), e);
            return 0;
        }
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            Some((meta.modified().ok()?, e.path()))
        })
        .collect();
    let target = (max_total as u64).saturating_sub(needed);
    if (files.len() as u64) <= target {
        return 0;
    }
    files.sort_by_key(|(mtime, _)| *mtime);
    let total = files.len() as u64;
    let mut evicted = 0u64;
    for (_, path) in files {
        if total.saturating_sub(evicted) <= target {
            break;
        }
        match fs::remove_file(&path) {
            Ok(()) => evicted += 1,
            Err(e) => log::debug!("failure snapshot: evict {} failed: {}", path.display(), e),
        }
    }
    evicted
}

/// 以受限权限写文件：unix 下 0600（快照含目标站响应原文，属敏感数据）；
/// 其他平台退化为进程默认 umask
#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}

/// 以受限权限写文件（非 unix 平台：进程默认 umask）
#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use std::sync::LazyLock;

    static EMPTY_HEADERS: LazyLock<HashMap<String, String>> = LazyLock::new(HashMap::new);

    fn temp_snapshotter(max_total: usize) -> (FailureSnapshotter, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("snapshots");
        (FailureSnapshotter::new(dir.clone(), max_total), dir)
    }

    fn sample_input<'a>(
        url: &'a str,
        status: u16,
        reason: &'a str,
        content: &'a str,
        screenshot: Option<&'a str>,
    ) -> SnapshotInput<'a> {
        SnapshotInput {
            url,
            status_code: status,
            reason,
            content,
            headers: &EMPTY_HEADERS,
            screenshot,
        }
    }

    fn dir_file_count(dir: &Path) -> usize {
        fs::read_dir(dir).expect("read_dir").count()
    }

    fn read_meta(dir: &Path, key: &str) -> serde_json::Value {
        let raw = fs::read_to_string(dir.join(format!("{key}.meta.json"))).expect("meta.json");
        serde_json::from_str(&raw).expect("meta json parse")
    }

    // ========== 签名键 ==========

    /// 不同 reason 生成不同签名键（URL+status 相同）
    #[test]
    fn different_reason_yields_different_key() {
        let k1 = FailureSnapshotter::signature_key("https://x.com/a", 403, "cloudflare challenge");
        let k2 = FailureSnapshotter::signature_key("https://x.com/a", 403, "rate limited");
        assert_ne!(k1, k2);
        // 同输入键稳定
        assert_eq!(
            FailureSnapshotter::signature_key("https://x.com/a", 403, "cloudflare challenge"),
            k1
        );
    }

    /// 消息含路径分隔符/控制字符时键被消毒（不含 / \ 与不可打印字符）
    #[test]
    fn signature_key_sanitizes_path_separators_and_control_chars() {
        let key = FailureSnapshotter::signature_key(
            "https://x.com/a",
            403,
            "bad/../../etc\\passwd\n\t chall§enge",
        );
        assert!(!key.contains('/'), "key must not contain /: {key}");
        assert!(!key.contains('\\'), "key must not contain \\: {key}");
        assert!(
            key.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "key must be filename-safe: {key}"
        );
        // 键长 = 30 hex + ≤30 消毒消息
        assert!(key.chars().count() <= 60);
    }

    // ========== 去重存档 ==========

    /// 同签名两次 snapshot：目录内文件数不变，meta.json 的 count=2
    #[tokio::test]
    async fn same_signature_second_hit_keeps_file_count_and_updates_meta_count() {
        let (snapshotter, dir) = temp_snapshotter(200);
        let url = "https://x.com/challenge";
        let reason = "cloudflare challenge detected";
        snapshotter
            .snapshot(sample_input(url, 403, reason, "<html>cf</html>", None))
            .await;
        let key = FailureSnapshotter::signature_key(url, 403, reason);
        let count_after_first = dir_file_count(&dir);
        assert!(count_after_first >= 2, "first hit writes html + meta");

        snapshotter
            .snapshot(sample_input(url, 403, reason, "<html>cf</html>", None))
            .await;
        assert_eq!(
            dir_file_count(&dir),
            count_after_first,
            "second hit must not create new files"
        );
        assert_eq!(snapshotter.hit_count(&key), Some(2));
        assert_eq!(read_meta(&dir, &key)["count"], 2, "meta count must be 2");
    }

    /// 不同 reason 生成不同快照（各自落盘）
    #[tokio::test]
    async fn different_reason_writes_distinct_snapshots() {
        let (snapshotter, dir) = temp_snapshotter(200);
        snapshotter
            .snapshot(sample_input(
                "https://x.com/a",
                403,
                "reason-one",
                "<html>1</html>",
                None,
            ))
            .await;
        snapshotter
            .snapshot(sample_input(
                "https://x.com/a",
                403,
                "reason-two",
                "<html>2</html>",
                None,
            ))
            .await;
        assert!(dir
            .join(format!(
                "{}.html",
                FailureSnapshotter::signature_key("https://x.com/a", 403, "reason-one")
            ))
            .exists());
        assert!(dir
            .join(format!(
                "{}.html",
                FailureSnapshotter::signature_key("https://x.com/a", 403, "reason-two")
            ))
            .exists());
    }

    /// meta.json 携带 url / status / reason / headers
    #[tokio::test]
    async fn meta_json_carries_response_metadata() {
        let (snapshotter, dir) = temp_snapshotter(200);
        let mut headers = HashMap::new();
        headers.insert("server".to_string(), "cloudflare".to_string());
        let input = SnapshotInput {
            url: "https://x.com/meta",
            status_code: 429,
            reason: "rate limited",
            content: "<html>429</html>",
            headers: &headers,
            screenshot: None,
        };
        snapshotter.snapshot(input).await;
        let key = FailureSnapshotter::signature_key("https://x.com/meta", 429, "rate limited");
        let meta = read_meta(&dir, &key);
        assert_eq!(meta["url"], "https://x.com/meta");
        assert_eq!(meta["status"], 429);
        assert_eq!(meta["reason"], "rate limited");
        assert_eq!(meta["headers"]["server"], "cloudflare");
        assert_eq!(meta["count"], 1);
    }

    // ========== 磁盘护栏（滚动删除） ==========

    /// 达 max_total 后新签名滚动淘汰最旧文件腾位：目录文件数有界、新键可观测
    #[tokio::test]
    async fn max_total_rolls_oldest_files_to_admit_new_snapshot() {
        let (snapshotter, dir) = temp_snapshotter(2);
        let key_one = FailureSnapshotter::signature_key("https://x.com/one", 403, "r1");
        snapshotter
            .snapshot(sample_input(
                "https://x.com/one",
                403,
                "r1",
                "<html>1</html>",
                None,
            ))
            .await;
        assert_eq!(snapshotter.total_written(), 2, "html + meta = 2 files");
        assert!(dir.join(format!("{key_one}.html")).exists());

        // 护栏已满：新签名滚动删除最旧（key-one 的 html/meta），写入自身文件
        let key_two = FailureSnapshotter::signature_key("https://x.com/two", 403, "r2");
        snapshotter
            .snapshot(sample_input(
                "https://x.com/two",
                403,
                "r2",
                "<html>2</html>",
                None,
            ))
            .await;
        assert_eq!(
            dir_file_count(&dir),
            2,
            "rolling keeps directory bounded at max_total"
        );
        assert!(
            !dir.join(format!("{key_one}.html")).exists(),
            "oldest snapshot files evicted"
        );
        assert!(
            dir.join(format!("{key_two}.html")).exists(),
            "new snapshot admitted after eviction"
        );
        assert_eq!(snapshotter.hit_count(&key_two), Some(1), "new key counted");
        assert_eq!(
            snapshotter.total_written(),
            2,
            "cursor nets writes/evictions"
        );
    }

    /// 落盘文件权限收紧：unix 下 0600（敏感响应原文不得 world-readable）
    #[cfg(unix)]
    #[tokio::test]
    async fn snapshot_files_written_with_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (snapshotter, dir) = temp_snapshotter(200);
        snapshotter
            .snapshot(sample_input(
                "https://x.com/perm",
                403,
                "waf",
                "<html/>",
                None,
            ))
            .await;
        let key = FailureSnapshotter::signature_key("https://x.com/perm", 403, "waf");
        for name in [format!("{key}.html"), format!("{key}.meta.json")] {
            let mode = fs::metadata(dir.join(&name))
                .expect("snapshot file exists")
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "{name} must be owner-only, got {:o}",
                mode & 0o777
            );
        }
    }

    /// 签名键内存表达 [`MAX_TRACKED_KEYS`] 上限：超限后新键不入表（仅计指标），
    /// 既有键不受影响
    #[tokio::test]
    async fn counts_map_caps_distinct_keys() {
        let (snapshotter, _dir) = temp_snapshotter(1);
        let first_key = FailureSnapshotter::signature_key("https://x.com/k0", 403, "r");
        for i in 0..=MAX_TRACKED_KEYS {
            snapshotter
                .snapshot(sample_input(
                    &format!("https://x.com/k{i}"),
                    403,
                    "r",
                    "x",
                    None,
                ))
                .await;
        }
        assert_eq!(
            snapshotter.hit_count(&first_key),
            Some(1),
            "early key still tracked"
        );
        let overflow_key = FailureSnapshotter::signature_key(
            &format!("https://x.com/k{MAX_TRACKED_KEYS}"),
            403,
            "r",
        );
        assert_eq!(
            snapshotter.hit_count(&overflow_key),
            None,
            "key beyond cap must not enter the map"
        );
    }

    // ========== 截图透传 ==========

    /// screenshot=Some 时 .jpg 落盘且 base64 解码正确
    #[tokio::test]
    async fn screenshot_some_writes_decoded_jpg() {
        let (snapshotter, dir) = temp_snapshotter(200);
        // "aGVsbG8=" = "hello"
        snapshotter
            .snapshot(sample_input(
                "https://x.com/shot",
                403,
                "waf",
                "<html>shot</html>",
                Some("aGVsbG8="),
            ))
            .await;
        let key = FailureSnapshotter::signature_key("https://x.com/shot", 403, "waf");
        let jpg = fs::read(dir.join(format!("{key}.jpg"))).expect("jpg file");
        assert_eq!(jpg, b"hello", "jpg must be base64-decoded bytes");
        assert_eq!(snapshotter.total_written(), 3, "html + meta + jpg");
    }

    /// screenshot=None 时无 .jpg
    #[tokio::test]
    async fn screenshot_none_writes_no_jpg() {
        let (snapshotter, dir) = temp_snapshotter(200);
        snapshotter
            .snapshot(sample_input(
                "https://x.com/noshot",
                403,
                "waf",
                "<html>noshot</html>",
                None,
            ))
            .await;
        let key = FailureSnapshotter::signature_key("https://x.com/noshot", 403, "waf");
        assert!(!dir.join(format!("{key}.jpg")).exists());
        assert!(dir.join(format!("{key}.html")).exists());
    }

    /// 非法 base64 截图静默跳过（不 panic、不写 .jpg）
    #[tokio::test]
    async fn invalid_base64_screenshot_is_silently_skipped() {
        let (snapshotter, dir) = temp_snapshotter(200);
        snapshotter
            .snapshot(sample_input(
                "https://x.com/b64",
                403,
                "waf",
                "<html>b64</html>",
                Some("!!!not-base64!!!"),
            ))
            .await;
        let key = FailureSnapshotter::signature_key("https://x.com/b64", 403, "waf");
        assert!(
            !dir.join(format!("{key}.jpg")).exists(),
            "bad base64: no jpg"
        );
        assert!(
            dir.join(format!("{key}.html")).exists(),
            "html still written"
        );
        assert_eq!(snapshotter.total_written(), 2);
    }

    // ========== 错误显性化（旁路降级） ==========

    /// dir 不可写（路径为普通文件，create_dir_all 必败）时 snapshot 静默返回，无 panic
    #[tokio::test]
    async fn unwritable_dir_returns_silently_no_panic() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // 用普通文件路径冒充目录：create_dir_all 在任意用户（含 root）下都失败
        let file_path = tmp.path().join("not-a-dir");
        fs::write(&file_path, b"occupied").expect("write blocker file");
        let snapshotter = FailureSnapshotter::new(file_path.clone(), 200);

        snapshotter
            .snapshot(sample_input("https://x.com/x", 403, "r", "<html/>", None))
            .await;
        assert_eq!(snapshotter.total_written(), 0, "nothing written");
        assert!(file_path.is_file(), "blocker file untouched");
    }

    // ========== 配置构造 ==========

    /// from_config：目录空串 = None（禁用）；非空 = Some
    #[test]
    fn from_config_empty_dir_is_none_and_populated_is_some() {
        let disabled = ObservabilitySettings {
            failure_snapshot_dir: String::new(),
            failure_snapshot_max_total: 200,
        };
        assert!(FailureSnapshotter::from_config(&disabled).is_none());

        let enabled = ObservabilitySettings {
            failure_snapshot_dir: "data/failure_snapshots".to_string(),
            failure_snapshot_max_total: 50,
        };
        let s = FailureSnapshotter::from_config(&enabled).expect("some");
        assert_eq!(s.total_written(), 0);
    }
}
