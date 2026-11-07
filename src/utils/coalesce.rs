// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 请求合并 Coalesce
//!
//! 移植 spider `coalesce.rs`：同一合并键的并发请求只允许首个执行实际抓取，
//! 其余 worker 等待首个完成后从缓存/DB 读取结果，避免重复网络往返。
//!
//! 核心数据结构：
//! - `CoalesceKey`：合并键 = 租户 + 规范化 URL + 完整请求身份指纹
//!   （`ScrapeOptions` 全字段，含 headers/proxy）
//! - `RequestCoalescer`：`DashMap<CoalesceKey, InFlightEntry>` 共享状态
//! - `CoalesceResult`：`try_start` 返回枚举（`Proceed`/`Wait`）
//! - `CoalesceGuard`：RAII guard，Drop 时广播完成通知并移除条目
//!
//! # 安全约束（M-2：跨租户内容泄露）
//!
//! 合并键必含租户维度。仅以 URL 为键时，租户 A 与租户 B 在等待窗口内请求
//! 同一 URL 会命中同一 in-flight 条目，等待方按 leader task_id 读到另一租户
//! 的抓取结果。租户 + 完整请求身份指纹进入键后：
//! - 不同租户永不合并（跨租户泄露路径不可达）
//! - 同租户但请求身份不同（headers/proxy/method/body/needs_js/... 任一
//!   `ScrapeOptions` 字段）也不合并（避免带认证头或不同渲染选项的请求互相串结果）
//! - 同租户且完整请求身份相同 → 合并
//!
//! 设计要点：
//! - `STALE_TIMEOUT = 120s`：超过该时长的 in-flight 条目视为僵死，`purge_stale` 清理
//! - `broadcast::channel(1)`：容量 1 即可，完成只发一次
//! - `CoalesceSignal`：广播载荷区分 `Completed`（leader 正常完成）与 `Purged`
//!   （僵死条目被清理），等待方据此决定是否按 leader task_id 查询结果
//! - `generation`：每次插入 in-flight 条目时单调递增。guard 与 `purge_stale`
//!   只删除 generation 匹配的条目，防止陈旧 guard / 过期清理快照误删同键新条目
//! - guard 持有 `Arc<DashMap>` 引用，不持有 `&RequestCoalescer`，避免生命周期耦合
//! - `CompactString` 改用 `String`（项目无 compact_str 依赖）

use crate::domain::id::Id;
use crate::engines::types::ScrapeOptions;
use crate::workers::cache_utils::redact_url_for_log;
use dashmap::DashMap;
use std::collections::HashMap;
use std::hash::Hasher;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// 僵死条目超时（秒）
pub const STALE_TIMEOUT: Duration = Duration::from_secs(120);

/// 请求合并键：租户 + 规范化 URL + 完整请求身份指纹
///
/// 三个维度共同决定"两个请求是否可以共用一次抓取结果"：
/// - `team_id`：租户隔离边界，跨租户永不合并（M-2 修复点）
/// - `url`：规范化后的目标 URL（等价写法归一）
/// - `fingerprint`：完整请求身份的确定性摘要（`ScrapeOptions` 全字段，
///   含 headers 与 proxy）
///
/// `Hash`/`Eq` 全维度比较，任一维度不同即视为不同请求。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CoalesceKey {
    team_id: Id,
    url: String,
    fingerprint: u64,
}

impl CoalesceKey {
    /// 构造合并键
    ///
    /// # 参数
    ///
    /// - `team_id`：任务所属租户（取自 `Task::team_id`）
    /// - `url`：目标 URL，构造时规范化（scheme/host 小写、去 fragment、去默认端口）
    /// - `options`：本次请求实际下发的完整选项（含 headers 与 proxy）
    ///
    /// # 指纹语义
    ///
    /// - **完整**：`ScrapeOptions` 全部字段参与指纹（method/body/needs_js/
    ///   session_id/mobile/needs_screenshot/screenshot_config/actions/wait_for/
    ///   block_ads/block_media/use_fire_engine/needs_mllm/skip_tls_verification/
    ///   headers/proxy 等），新增字段自动纳入
    /// - **确定性**：序列化为结构化 JSON 后递归按键排序，headers 名先小写再按
    ///   （名, 值）排序，`HashMap` 迭代序不影响指纹
    /// - **进程内稳定**：哈希用 `DefaultHasher`（SipHash-1-3）。算法跨 Rust 版本
    ///   不保证不变，但合并表只存在于进程内存，进程重启即清空，无须跨进程稳定
    /// - **空输入稳定**：默认 `ScrapeOptions` 得到固定指纹，等价请求必然合并
    /// - **值敏感**：header 值保持大小写敏感（`Bearer A` 与 `bearer a` 不合并）
    ///
    /// # 碰撞边界
    ///
    /// 64 位指纹理论上可碰撞，受影响范围仅限**同一租户**内两个请求身份不同的
    /// 请求被合并；租户维度独立参与键比较，跨租户隔离不受指纹碰撞影响。
    pub fn new(team_id: Id, url: &str, options: &ScrapeOptions) -> Self {
        Self {
            team_id,
            url: normalize_coalesce_url(url),
            fingerprint: request_fingerprint(options),
        }
    }

    /// 租户 ID
    pub fn team_id(&self) -> Id {
        self.team_id
    }

    /// 规范化后的 URL
    pub fn url(&self) -> &str {
        &self.url
    }

    /// 请求指纹（完整请求身份的稳定摘要）
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

/// 规范化 URL：只做无歧义归一，不改写 path/query 语义
///
/// `url::Url` 解析会小写 scheme/host、去掉默认端口（`:80`/`:443`）并按 RFC 3986
/// 规范化百分号编码；此处额外去 fragment（服务端不感知 fragment）。
///
/// 刻意不做的归一：剥 utm 参数、去 trailing slash、切换 http/https。这些改写会
/// 让语义可能不同的 URL 合并，属于"错合并"，与本模块的保守取舍相反。
///
/// 解析失败时回退为 `trim()` 原串：调用方（scrape 链路）已做 http/https 校验，
/// 回退保证异常串仍能稳定命中自身，不静默丢弃条目。
fn normalize_coalesce_url(raw: &str) -> String {
    match url::Url::parse(raw.trim()) {
        Ok(mut parsed) => {
            parsed.set_fragment(None);
            parsed.to_string()
        }
        Err(_) => raw.trim().to_string(),
    }
}

/// 计算完整请求身份（`ScrapeOptions` 全字段）的确定性指纹
///
/// 编码分两步：
/// 1. `serde_json::to_value` 把 `ScrapeOptions` 转成结构化 JSON 表示；headers
///    单独替换为「小写名 + 值排序的二元组数组」，消除 `HashMap` 迭代序与
///    header 名大小写差异
/// 2. 递归按键排序所有 JSON 对象后 `serde_json::to_vec` 得到确定性字节串，
///    再经 `DefaultHasher`（SipHash-1-3）压缩为 64 位
///
/// 选 `DefaultHasher` 而非自研 FNV：键只存在于进程内存，无须跨进程稳定；
/// 结构化序列化已保证编码确定，标准库零依赖即可。
fn request_fingerprint(options: &ScrapeOptions) -> u64 {
    // 字段名必须与 `ScrapeOptions::headers` 一致；该字段改名时须同步更新此处，
    // 否则 headers 将被错误地按 `HashMap` 迭代序编码。
    const HEADERS_FIELD: &str = "headers";

    let mut identity =
        serde_json::to_value(options).expect("ScrapeOptions JSON serialization is infallible");
    if let serde_json::Value::Object(map) = &mut identity {
        map.insert(
            HEADERS_FIELD.to_string(),
            canonical_headers(&options.headers),
        );
    }
    let canonical = canonicalize_json(identity);
    let bytes =
        serde_json::to_vec(&canonical).expect("canonical JSON value serialization is infallible");

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(&bytes);
    hasher.finish()
}

/// 把 headers 编码为「小写名 + 值」排序的二元组数组
///
/// 先小写名（RFC 9110 大小写不敏感），再按（名, 值）排序：同名不同大小写的
/// header 也能得到与 `HashMap` 迭代序无关的确定编码。
fn canonical_headers(headers: &HashMap<String, String>) -> serde_json::Value {
    use serde_json::Value;

    let mut pairs: Vec<(String, &str)> = headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.as_str()))
        .collect();
    pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    Value::Array(
        pairs
            .into_iter()
            .map(|(name, value)| {
                Value::Array(vec![Value::String(name), Value::String(value.to_owned())])
            })
            .collect(),
    )
}

/// 递归按键排序 JSON 对象（数组保持原序）
///
/// `serde_json` 启用 `preserve_order` 时 `Map` 保留插入序；本函数显式重建
/// 有序对象，使编码不依赖字段插入顺序或 `HashMap` 迭代序。
fn canonicalize_json(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;

    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            let mut sorted = serde_json::Map::with_capacity(entries.len());
            for (key, value) in entries {
                sorted.insert(key, canonicalize_json(value));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize_json).collect()),
        scalar => scalar,
    }
}

/// coalesce 广播信号：区分 leader 正常完成与 stale 清理
///
/// 等待方据此决定后续动作：
/// - [`Completed`](Self::Completed)：leader guard Drop，抓取正常完成 → 按 leader task_id 查询结果
/// - [`Purged`](Self::Purged)：`purge_stale` 清理僵死条目（leader 可能 panic/死锁）→
///   不视为完成，走自身重排
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoalesceSignal {
    /// leader 抓取正常完成（guard Drop 广播）
    Completed,
    /// in-flight 条目被 `purge_stale` 判定僵死并清理（leader 异常）
    Purged,
}

/// In-flight 条目：记录开始时间 + leader 任务 ID + 完成广播 sender + 条目代号
#[derive(Debug)]
struct InFlightEntry {
    started_at: Instant,
    /// leader（首个获得执行权的 worker）的任务 ID。
    /// 等待方按此 ID 查询落库结果——结果落在 leader 名下，非等待方自身 task_id。
    leader_task_id: Id,
    sender: broadcast::Sender<CoalesceSignal>,
    /// 条目代号：每次插入时从 `RequestCoalescer` 的单调计数器取新值。
    /// guard Drop 与 `purge_stale` 只删除 generation 匹配的条目，防止陈旧
    /// guard / 过期清理快照误删同键的新条目。
    generation: u64,
}

/// `try_start` 返回结果
#[derive(Debug)]
pub enum CoalesceResult {
    /// 调用方获得执行权，可继续实际抓取；guard Drop 后广播 [`CoalesceSignal::Completed`]
    Proceed(CoalesceGuard),
    /// 已有同合并键的请求在执行，调用方应等待广播信号后按 leader task_id 读取结果。
    /// 元组第二项为 leader（首个执行方）的 task_id，等待方据此查询落库结果。
    Wait(broadcast::Receiver<CoalesceSignal>, Id),
}

/// RAII guard：持有期间该合并键被标记为 in-flight
///
/// Drop 时按 generation 匹配移除条目并广播 [`CoalesceSignal::Completed`] 通知所有等待方。
/// 调用方可显式 [`release`](Self::release) 以提前释放（语义等价于 drop）。
#[derive(Debug)]
pub struct CoalesceGuard {
    key: CoalesceKey,
    /// 本 guard 插入条目时的代号（见 [`InFlightEntry::generation`]）
    generation: u64,
    in_flight: Arc<DashMap<CoalesceKey, InFlightEntry>>,
}

impl CoalesceGuard {
    /// 显式释放：等价于 `drop(self)`
    ///
    /// 提供语义清晰的 API，便于调用方表达"抓取已完成"的意图。
    /// 命名 `release` 避免与 `Complete` trait 方法歧义。
    pub fn release(self) {
        // Drop 处理实际清理与广播
        drop(self);
    }

    /// 获取 guard 对应的规范化 URL（测试与日志用）
    pub fn url(&self) -> &str {
        self.key.url()
    }

    /// 获取 guard 对应的合并键（测试与日志用）
    pub fn key(&self) -> &CoalesceKey {
        &self.key
    }
}

impl Drop for CoalesceGuard {
    fn drop(&mut self) {
        // 仅删除自己 generation 的条目：若条目已被 purge_stale 清理且同键已有
        // 新 guard 插入，旧 guard 的 Drop 不得删除新条目，也不得向新条目的
        // 等待方广播 Completed。
        //
        // 顺序保持「先移除匹配条目，再广播」：remove_if 返回拥有所有权的条目，
        // 广播不持有 DashMap 锁。
        if let Some((_, entry)) = self
            .in_flight
            .remove_if(&self.key, |_, entry| entry.generation == self.generation)
        {
            if let Err(e) = entry.sender.send(CoalesceSignal::Completed) {
                log::debug!(
                    "Coalesce broadcast failed for URL {}: {} (no active waiters)",
                    redact_url_for_log(self.key.url()),
                    e
                );
            }
        }
    }
}

/// 请求合并器：共享 `DashMap` 追踪 in-flight 合并键
///
/// 线程安全：内部 `Arc<DashMap>`，可廉价克隆共享。
/// 放入 `CrawlRsState` 供所有 worker 共享同一实例。
#[derive(Debug, Clone)]
pub struct RequestCoalescer {
    in_flight: Arc<DashMap<CoalesceKey, InFlightEntry>>,
    /// 条目代号单调计数器（所有 clone 共享）
    next_generation: Arc<AtomicU64>,
}

impl Default for RequestCoalescer {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestCoalescer {
    /// 创建新的空合并器
    pub fn new() -> Self {
        Self {
            in_flight: Arc::new(DashMap::new()),
            next_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 取下一个条目代号（单调递增，进程内唯一）
    fn next_generation(&self) -> u64 {
        self.next_generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// 尝试获取该合并键的执行权
    ///
    /// - 首个调用方返回 [`CoalesceResult::Proceed`]，获得 guard；`leader_task_id`
    ///   被记入 in-flight 条目，供后续等待方按此 ID 查询落库结果
    /// - 后续调用方返回 [`CoalesceResult::Wait`]，携带 leader 的 task_id，
    ///   应 `await` receiver 得到 [`CoalesceSignal`] 后按 leader task_id 读取结果
    ///
    /// 合并粒度由 [`CoalesceKey`] 决定：租户、URL、完整请求身份指纹全部相同
    /// 才命中同一 entry。
    ///
    /// 并发安全：使用 `DashMap::entry` 原子插入，避免 TOCTOU。
    ///
    /// # 性能
    ///
    /// `broadcast::channel(1)` 仅在 `Vacant` 分支（Proceed 路径）构造，
    /// 避免在 `Occupied`（Wait 路径）做无谓分配。
    pub fn try_start(&self, key: &CoalesceKey, leader_task_id: Id) -> CoalesceResult {
        use dashmap::mapref::entry::Entry;

        match self.in_flight.entry(key.clone()) {
            Entry::Occupied(o) => {
                // Wait 路径：订阅现有 sender 并读取 leader 的 task_id（零额外分配）
                let entry = o.get();
                let rx = entry.sender.subscribe();
                CoalesceResult::Wait(rx, entry.leader_task_id)
            }
            Entry::Vacant(v) => {
                // Proceed 路径：此时才分配 channel（容量 1 即可）
                let (tx, _rx) = broadcast::channel(1);
                let generation = self.next_generation();
                let entry = InFlightEntry {
                    started_at: Instant::now(),
                    leader_task_id,
                    sender: tx,
                    generation,
                };
                v.insert(entry);
                CoalesceResult::Proceed(CoalesceGuard {
                    key: key.clone(),
                    generation,
                    in_flight: self.in_flight.clone(),
                })
            }
        }
    }

    /// 清理超过 [`STALE_TIMEOUT`] 的僵死条目
    ///
    /// 僵死条目（worker panic / 死锁导致 guard 未 Drop）会阻塞相同合并键的后续请求，
    /// 定期调用本方法回收。清理时广播 [`CoalesceSignal::Purged`] 通知等待方
    /// （等待方不视为完成，走自身重排）。
    ///
    /// 返回实际清理的条目数。删除按 `(key, generation)` 条件执行：收集快照后
    /// 条目可能已被 guard Drop 移除并重新插入，generation 不匹配时保留新条目。
    pub fn purge_stale(&self) -> usize {
        let now = Instant::now();
        let stale: Vec<(CoalesceKey, u64)> = self
            .in_flight
            .iter()
            .filter(|entry| now.duration_since(entry.started_at) > STALE_TIMEOUT)
            .map(|entry| (entry.key().clone(), entry.generation))
            .collect();

        let mut purged = 0;
        for (key, generation) in stale {
            if let Some((_, entry)) = self
                .in_flight
                .remove_if(&key, |_, entry| entry.generation == generation)
            {
                purged += 1;
                // 广播 Purged：等待方不视为完成，走自身重排
                let _ = entry.sender.send(CoalesceSignal::Purged);
            }
        }
        purged
    }

    /// 当前 in-flight 条目数（测试与监控用）
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    /// 判断合并键是否 in-flight（测试用）
    pub fn is_in_flight(&self, key: &CoalesceKey) -> bool {
        self.in_flight.contains_key(key)
    }

    /// 测试专用：强制清理指定合并键条目并广播 [`CoalesceSignal::Purged`]
    /// （不论是否超时），用于验证等待方对 Purged 信号的处理。
    #[cfg(test)]
    pub(crate) fn purge_key(&self, key: &CoalesceKey) {
        if let Some((_, entry)) = self.in_flight.remove(key) {
            let _ = entry.sender.send(CoalesceSignal::Purged);
        }
    }

    /// 测试专用：移除条目但不广播（sender 随条目 drop），
    /// 用于模拟 leader 异常消失导致等待方 `recv()` 返回 `Closed`。
    #[cfg(test)]
    pub(crate) fn drop_entry_silently(&self, key: &CoalesceKey) {
        self.in_flight.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{CacheMode, HttpMethod};
    use crate::engines::types::{PageAction, ScreenshotConfig, WaitFor};
    use std::sync::Arc;
    use std::thread;

    /// 测试固定租户：同一用例内构造的键共享租户维度
    fn test_team() -> Id {
        Id::from_u128(0x7e57_0000_0000_0000_0000_0000_0000_0001)
    }

    /// 默认键：固定租户 + 给定 URL + 默认 `ScrapeOptions`
    fn key(url: &str) -> CoalesceKey {
        CoalesceKey::new(test_team(), url, &ScrapeOptions::default())
    }

    /// 给定选项构造固定租户的键
    fn key_with(url: &str, options: &ScrapeOptions) -> CoalesceKey {
        CoalesceKey::new(test_team(), url, options)
    }

    /// 构造只含指定 headers 的选项
    fn options_with_headers(pairs: &[(&str, &str)]) -> ScrapeOptions {
        ScrapeOptions {
            headers: pairs
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect(),
            ..Default::default()
        }
    }

    /// 首个 try_start 应返回 Proceed
    #[test]
    fn try_start_first_call_returns_proceed() {
        let coalescer = RequestCoalescer::new();
        let result = coalescer.try_start(&key("https://example.com/page1"), Id::generate());
        assert!(matches!(result, CoalesceResult::Proceed(_)));
        assert!(coalescer.is_in_flight(&key("https://example.com/page1")));
    }

    /// 同键第二个 try_start 应返回 Wait
    #[test]
    fn try_start_second_call_same_url_returns_wait() {
        let coalescer = RequestCoalescer::new();
        let _guard = coalescer.try_start(&key("https://example.com/page2"), Id::generate());
        let result = coalescer.try_start(&key("https://example.com/page2"), Id::generate());
        assert!(matches!(result, CoalesceResult::Wait(..)));
    }

    /// 不同 URL 应各自获得 Proceed
    #[test]
    fn try_start_different_urls_both_proceed() {
        let coalescer = RequestCoalescer::new();
        let _g1 = coalescer.try_start(&key("https://a.com"), Id::generate());
        let _g2 = coalescer.try_start(&key("https://b.com"), Id::generate());
        assert_eq!(coalescer.in_flight_count(), 2);
    }

    /// guard Drop 后该键可再次 Proceed（条目已移除）
    #[test]
    fn guard_drop_removes_entry_and_allows_retry() {
        let coalescer = RequestCoalescer::new();
        {
            let _guard = coalescer.try_start(&key("https://example.com/temp"), Id::generate());
            assert!(coalescer.is_in_flight(&key("https://example.com/temp")));
        }
        // guard 已 Drop
        assert!(!coalescer.is_in_flight(&key("https://example.com/temp")));
        // 可再次获取
        let result = coalescer.try_start(&key("https://example.com/temp"), Id::generate());
        assert!(matches!(result, CoalesceResult::Proceed(_)));
    }

    /// release 语义等价于 drop
    #[test]
    fn release_is_equivalent_to_drop() {
        let coalescer = RequestCoalescer::new();
        let guard = match coalescer.try_start(&key("https://example.com/release"), Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        assert!(coalescer.is_in_flight(&key("https://example.com/release")));
        guard.release();
        assert!(!coalescer.is_in_flight(&key("https://example.com/release")));
    }

    /// guard url 返回规范化后的 URL
    #[test]
    fn guard_url_returns_correct_url() {
        let coalescer = RequestCoalescer::new();
        let guard = match coalescer.try_start(&key("https://example.com/url-test"), Id::generate())
        {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        assert_eq!(guard.url(), "https://example.com/url-test");
    }

    /// guard key 与传入键一致
    #[test]
    fn guard_key_matches_inserted_key() {
        let coalescer = RequestCoalescer::new();
        let inserted = key("https://example.com/key-test");
        let guard = match coalescer.try_start(&inserted, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        assert_eq!(guard.key(), &inserted);
        assert_eq!(guard.key().team_id(), test_team());
    }

    /// purge_stale 清理超时条目并返回清理数量
    #[test]
    fn purge_stale_removes_timed_out_entries() {
        // 直接构造一个过时条目（模拟 started_at 很早）
        let coalescer = RequestCoalescer::new();
        let (tx, _rx) = broadcast::channel(1);
        coalescer.in_flight.insert(
            key("https://example.com/stale"),
            InFlightEntry {
                started_at: Instant::now() - Duration::from_secs(200),
                leader_task_id: Id::generate(),
                sender: tx,
                generation: coalescer.next_generation(),
            },
        );
        // 还插入一个未过时的
        let guard = match coalescer.try_start(&key("https://example.com/fresh"), Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };

        assert_eq!(coalescer.in_flight_count(), 2);
        let purged = coalescer.purge_stale();
        assert_eq!(purged, 1, "should purge 1 stale entry");
        assert!(!coalescer.is_in_flight(&key("https://example.com/stale")));
        assert!(coalescer.is_in_flight(&key("https://example.com/fresh")));

        // fresh 的 guard 还有效
        guard.release();
    }

    /// purge_stale 无超时条目时返回 0
    #[test]
    fn purge_stale_returns_zero_when_no_stale() {
        let coalescer = RequestCoalescer::new();
        let _guard = coalescer.try_start(&key("https://example.com/active"), Id::generate());
        assert_eq!(coalescer.purge_stale(), 0);
        assert!(coalescer.is_in_flight(&key("https://example.com/active")));
    }

    /// purge_stale 在空合并器上返回 0
    #[test]
    fn purge_stale_empty_returns_zero() {
        let coalescer = RequestCoalescer::new();
        assert_eq!(coalescer.purge_stale(), 0);
    }

    /// 并发 try_start 同一键只有一个 Proceed
    ///
    /// 使用多线程 + barrier 模拟 worker 并发，验证只有一个获得 Proceed。
    /// 关键：guard 必须保留到所有线程都调用 try_start 之后，否则 guard Drop
    /// 会让后续线程也获得 Proceed。
    #[test]
    fn concurrent_try_start_same_url_only_one_proceed() {
        let coalescer = Arc::new(RequestCoalescer::new());
        let url = "https://example.com/concurrent";
        let proceeds = Arc::new(std::sync::Mutex::new(0usize));
        let waits = Arc::new(std::sync::Mutex::new(0usize));
        // 收集 guard 防止过早 Drop
        let guards: Arc<std::sync::Mutex<Vec<CoalesceGuard>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let barrier = Arc::new(std::sync::Barrier::new(8));

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let c = coalescer.clone();
                let p = proceeds.clone();
                let w = waits.clone();
                let g = guards.clone();
                let b = barrier.clone();
                thread::spawn(move || {
                    // 等待所有线程就绪后同时调用 try_start
                    b.wait();
                    let result = c.try_start(&key(url), Id::generate());
                    match result {
                        CoalesceResult::Proceed(guard) => {
                            g.lock().unwrap().push(guard);
                            *p.lock().unwrap() += 1;
                        }
                        CoalesceResult::Wait(..) => *w.lock().unwrap() += 1,
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked");
        }

        assert_eq!(*proceeds.lock().unwrap(), 1, "exactly one Proceed");
        assert_eq!(*waits.lock().unwrap(), 7, "seven Wait");
        // guards 在此处 Drop，清理 in_flight
    }

    /// Default 等价于 new
    #[test]
    fn default_equals_new() {
        let a = RequestCoalescer::new();
        let b = RequestCoalescer::default();
        assert_eq!(a.in_flight_count(), 0);
        assert_eq!(b.in_flight_count(), 0);
    }

    /// Clone 后共享同一 DashMap（修改互相可见）
    #[test]
    fn clone_shares_in_flight_state() {
        let a = RequestCoalescer::new();
        let b = a.clone();
        let _guard = a.try_start(&key("https://example.com/shared"), Id::generate());
        // b 也能看到
        assert!(b.is_in_flight(&key("https://example.com/shared")));
        assert_eq!(b.in_flight_count(), 1);
    }

    /// 等待方在 guard Drop 后能收到广播（异步验证）
    #[tokio::test]
    async fn waiter_receives_broadcast_after_guard_drop() {
        let coalescer = Arc::new(RequestCoalescer::new());
        let url = "https://example.com/async-wait";

        let guard = match coalescer.try_start(&key(url), Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };

        let mut rx = match coalescer.try_start(&key(url), Id::generate()) {
            CoalesceResult::Wait(rx, _) => rx,
            _ => panic!("expected Wait"),
        };

        // 在异步任务中 Drop guard
        let coalescer_clone = coalescer.clone();
        tokio::spawn(async move {
            // 稍后 Drop
            tokio::time::sleep(Duration::from_millis(50)).await;
            drop(guard);
            let _ = coalescer_clone;
        });

        // 等待方应收到 Completed 广播
        let signal = rx.recv().await.expect("waiter should receive broadcast");
        assert_eq!(signal, CoalesceSignal::Completed);
    }

    /// purge_stale 广播 Purged 信号：等待方收到 Purged 而非 Completed
    #[tokio::test]
    async fn purge_stale_broadcasts_purged_signal() {
        let coalescer = RequestCoalescer::new();
        // 构造已超时的僵死条目（leader 异常未 Drop guard）
        let (tx, _rx) = broadcast::channel(1);
        coalescer.in_flight.insert(
            key("https://example.com/stale-signal"),
            InFlightEntry {
                started_at: Instant::now() - Duration::from_secs(200),
                leader_task_id: Id::generate(),
                sender: tx,
                generation: coalescer.next_generation(),
            },
        );
        let mut rx =
            match coalescer.try_start(&key("https://example.com/stale-signal"), Id::generate()) {
                CoalesceResult::Wait(rx, _) => rx,
                _ => panic!("expected Wait"),
            };
        // purge 触发广播
        assert_eq!(coalescer.purge_stale(), 1);
        // 等待方应收到 Purged（非 Completed）——不视为完成
        let signal = rx.recv().await.expect("should receive purge signal");
        assert_eq!(signal, CoalesceSignal::Purged);
    }

    /// Wait 路径携带 leader task_id：等待方读取到首个执行方的 task_id
    #[test]
    fn wait_carries_leader_task_id() {
        let coalescer = RequestCoalescer::new();
        let leader_id = Id::generate();
        let url = "https://example.com/leader-id";
        let _guard = match coalescer.try_start(&key(url), leader_id) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        // 等待方应读到 leader 的 task_id（而非自己）
        let waiter_id = Id::generate();
        match coalescer.try_start(&key(url), waiter_id) {
            CoalesceResult::Wait(_rx, got_leader_id) => {
                assert_eq!(got_leader_id, leader_id, "waiter should see leader task_id");
                assert_ne!(got_leader_id, waiter_id);
            }
            _ => panic!("expected Wait"),
        }
    }

    /// purge_key 不论条目是否超时都强制清理并广播 Purged（测试辅助方法自证）
    #[tokio::test]
    async fn purge_key_broadcasts_purged_regardless_of_age() {
        let coalescer = RequestCoalescer::new();
        let url = "https://example.com/force-purge";
        let _guard = match coalescer.try_start(&key(url), Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        let mut rx = match coalescer.try_start(&key(url), Id::generate()) {
            CoalesceResult::Wait(rx, _) => rx,
            _ => panic!("expected Wait"),
        };
        // 条目未超时，但 purge_key 强制清理
        coalescer.purge_key(&key(url));
        assert!(!coalescer.is_in_flight(&key(url)));
        let signal = rx.recv().await.expect("should receive signal");
        assert_eq!(signal, CoalesceSignal::Purged);
    }

    /// drop_entry_silently 移除条目不广播 → 订阅方 recv 返回 Closed（测试辅助方法自证）
    #[tokio::test]
    async fn drop_entry_silently_causes_closed() {
        let coalescer = RequestCoalescer::new();
        let url = "https://example.com/silent";
        let _guard = match coalescer.try_start(&key(url), Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        let mut rx = match coalescer.try_start(&key(url), Id::generate()) {
            CoalesceResult::Wait(rx, _) => rx,
            _ => panic!("expected Wait"),
        };
        coalescer.drop_entry_silently(&key(url));
        assert!(!coalescer.is_in_flight(&key(url)));
        // sender 随条目 drop，无广播 → recv 返回 Closed
        let err = rx.recv().await.expect_err("channel should be closed");
        assert!(matches!(err, broadcast::error::RecvError::Closed));
    }

    // === 合并键维度测试（M-2 / H1） ===

    /// 同租户 + 同 URL + 相同请求身份 → 合并命中
    ///
    /// 两个键分别由内容相同、插入顺序不同的 headers 构造，
    /// 验证指纹顺序无关且合并行为正确。
    #[test]
    fn same_tenant_same_fingerprint_coalesces() {
        let coalescer = RequestCoalescer::new();
        let team = Id::generate();
        let url = "https://example.com/coalesce-hit";

        let mut options_a = options_with_headers(&[("X-Api-Key", "k1"), ("Accept", "text/html")]);
        options_a.proxy = Some("http://proxy:8080".to_string());
        let mut options_b = options_with_headers(&[("Accept", "text/html"), ("X-Api-Key", "k1")]);
        options_b.proxy = Some("http://proxy:8080".to_string());

        let leader_key = CoalesceKey::new(team, url, &options_a);
        let waiter_key = CoalesceKey::new(team, url, &options_b);
        assert_eq!(
            leader_key, waiter_key,
            "header insertion order must not change the key"
        );

        let _guard = match coalescer.try_start(&leader_key, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        assert!(
            matches!(
                coalescer.try_start(&waiter_key, Id::generate()),
                CoalesceResult::Wait(..)
            ),
            "same tenant + same fingerprint must coalesce"
        );
        assert_eq!(coalescer.in_flight_count(), 1);
    }

    /// 不同 team_id 即使 URL 与请求选项全同也不合并（M-2 核心断言）
    #[test]
    fn different_team_id_does_not_coalesce() {
        let coalescer = RequestCoalescer::new();
        let url = "https://example.com/tenant-split";
        let key_a = CoalesceKey::new(Id::generate(), url, &ScrapeOptions::default());
        let key_b = CoalesceKey::new(Id::generate(), url, &ScrapeOptions::default());
        assert_ne!(key_a, key_b);

        let _guard_a = match coalescer.try_start(&key_a, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed for tenant A"),
        };
        let _guard_b = match coalescer.try_start(&key_b, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            CoalesceResult::Wait(..) => {
                panic!("tenant B must not wait on tenant A's in-flight entry")
            }
        };
        assert_eq!(coalescer.in_flight_count(), 2);
    }

    /// 跨租户 + 不同 headers 的完整泄露场景：B 不得拿到 A 的结果
    #[test]
    fn cross_tenant_different_headers_never_adopts_leader_result() {
        let coalescer = RequestCoalescer::new();
        let url = "https://example.com/private";

        let options_a = options_with_headers(&[("Authorization", "Bearer tenant-a")]);
        let options_b = options_with_headers(&[("Authorization", "Bearer tenant-b")]);

        let key_a = CoalesceKey::new(Id::generate(), url, &options_a);
        let key_b = CoalesceKey::new(Id::generate(), url, &options_b);

        let _guard_a = match coalescer.try_start(&key_a, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed for tenant A"),
        };
        let _guard_b = match coalescer.try_start(&key_b, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            CoalesceResult::Wait(..) => {
                panic!("tenant B adopted tenant A's leader instead of proceeding")
            }
        };
    }

    /// 同租户但 headers 不同不合并
    #[test]
    fn different_headers_do_not_coalesce() {
        let coalescer = RequestCoalescer::new();
        let team = Id::generate();
        let url = "https://example.com/headers-split";

        let options_a = options_with_headers(&[("X-Trace", "a")]);
        let options_b = options_with_headers(&[("X-Trace", "b")]);

        let key_a = CoalesceKey::new(team, url, &options_a);
        let key_b = CoalesceKey::new(team, url, &options_b);
        assert_ne!(key_a.fingerprint(), key_b.fingerprint());

        let _guard_a = match coalescer.try_start(&key_a, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        let _guard_b = match coalescer.try_start(&key_b, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            CoalesceResult::Wait(..) => panic!("different headers must not coalesce"),
        };
        assert_eq!(coalescer.in_flight_count(), 2);
    }

    /// 同租户 + 同 headers 但 proxy 不同不合并
    #[test]
    fn different_proxy_do_not_coalesce() {
        let coalescer = RequestCoalescer::new();
        let team = Id::generate();
        let url = "https://example.com/proxy-split";

        let options_a = ScrapeOptions {
            proxy: Some("http://proxy-a:8080".to_string()),
            ..Default::default()
        };
        let options_b = ScrapeOptions {
            proxy: Some("http://proxy-b:8080".to_string()),
            ..Default::default()
        };
        let options_empty = ScrapeOptions {
            proxy: Some(String::new()),
            ..Default::default()
        };

        let no_proxy = CoalesceKey::new(team, url, &ScrapeOptions::default());
        let proxy_a = CoalesceKey::new(team, url, &options_a);
        let proxy_b = CoalesceKey::new(team, url, &options_b);
        let empty_proxy = CoalesceKey::new(team, url, &options_empty);
        assert_ne!(no_proxy, proxy_a);
        assert_ne!(proxy_a, proxy_b);
        assert_ne!(no_proxy, empty_proxy);

        let _guard_a = match coalescer.try_start(&proxy_a, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        let _guard_b = match coalescer.try_start(&proxy_b, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            CoalesceResult::Wait(..) => panic!("different proxy must not coalesce"),
        };
        assert_eq!(coalescer.in_flight_count(), 2);
    }

    /// 默认选项的指纹稳定（跨调用/跨 map 实例一致）
    #[test]
    fn empty_headers_and_proxy_fingerprint_is_stable() {
        let team = test_team();
        let url = "https://example.com/empty";
        let first = CoalesceKey::new(team, url, &ScrapeOptions::default());
        let second = CoalesceKey::new(team, url, &ScrapeOptions::default());
        assert_eq!(first.fingerprint(), second.fingerprint());
        assert_eq!(first, second);
        // 显式构造的空 header map 与默认空 map 一致
        let explicit_empty = ScrapeOptions {
            headers: HashMap::with_capacity(4),
            ..Default::default()
        };
        assert_eq!(
            first,
            CoalesceKey::new(team, url, &explicit_empty),
            "empty header maps must produce the same fingerprint"
        );
    }

    /// 常见等价 URL 写法归一到同一键；query 语义不同则保持分离
    #[test]
    fn url_normalization_groups_equivalent_forms() {
        let team = test_team();
        let options = ScrapeOptions::default();
        let base = CoalesceKey::new(team, "https://example.com/page", &options);

        assert_eq!(
            base,
            CoalesceKey::new(team, "HTTPS://Example.COM/page#section", &options),
            "scheme/host case and fragment must not change the key"
        );
        assert_eq!(
            base,
            CoalesceKey::new(team, "https://example.com:443/page", &options),
            "default port must not change the key"
        );
        assert_ne!(
            base,
            CoalesceKey::new(team, "https://example.com/page?a=1", &options),
            "query strings must stay part of the key"
        );
        assert_ne!(
            base,
            CoalesceKey::new(team, "http://example.com/page", &options),
            "scheme must stay part of the key"
        );
    }

    /// header 名大小写归一（HTTP 语义等价，合并正确）
    #[test]
    fn header_name_case_is_normalized() {
        let team = test_team();
        let url = "https://example.com/case";
        let lower = options_with_headers(&[("x-token", "v")]);
        let mixed = options_with_headers(&[("X-Token", "v")]);
        assert_eq!(
            CoalesceKey::new(team, url, &lower),
            CoalesceKey::new(team, url, &mixed),
            "header names are case-insensitive per RFC 9110"
        );
        // 值仍大小写敏感：不同的认证令牌不得合并
        let lower_value = options_with_headers(&[("X-Token", "V")]);
        assert_ne!(
            CoalesceKey::new(team, url, &mixed),
            CoalesceKey::new(team, url, &lower_value),
            "header values must stay case-sensitive"
        );
    }

    /// 同名不同大小写的 header 与插入顺序无关（L1：先小写再排序）
    #[test]
    fn mixed_case_header_names_are_order_independent() {
        let url = "https://example.com/case-duplicates";
        let first = options_with_headers(&[
            ("Accept", "text/html"),
            ("accept", "application/json"),
            ("X-Trace", "1"),
        ]);
        let second = options_with_headers(&[
            ("x-trace", "1"),
            ("accept", "application/json"),
            ("ACCEPT", "text/html"),
        ]);
        assert_eq!(
            CoalesceKey::new(test_team(), url, &first),
            CoalesceKey::new(test_team(), url, &second),
            "lowercased header names must be sorted deterministically"
        );
    }

    /// 结构化编码无歧义：("ab","c") 与 ("a","bc") 指纹不同
    #[test]
    fn fingerprint_encoding_is_unambiguous() {
        let team = test_team();
        let url = "https://example.com/encoding";
        let split_a = options_with_headers(&[("ab", "c")]);
        let split_b = options_with_headers(&[("a", "bc")]);
        assert_ne!(
            CoalesceKey::new(team, url, &split_a).fingerprint(),
            CoalesceKey::new(team, url, &split_b).fingerprint()
        );
    }

    /// 指纹进程内稳定：相同输入重复构造得到相同键
    #[test]
    fn fingerprint_is_stable_within_process() {
        let team = Id::from_u128(1);
        let options = ScrapeOptions {
            proxy: Some("http://p:1".to_string()),
            ..Default::default()
        };
        let key = CoalesceKey::new(team, "https://example.com/fixed", &options);
        // 只断言同一输入重复构造稳定，不断言具体数值（避免测试耦合实现细节）
        let again = CoalesceKey::new(team, "https://example.com/fixed", &options);
        assert_eq!(key.fingerprint(), again.fingerprint());
        assert_eq!(key, again);
    }

    /// H1 回归：指纹覆盖 `ScrapeOptions` 全字段
    ///
    /// 任一影响实际请求/响应内容的字段变化都必须产生不同指纹；否则同租户同 URL
    /// 但选项不同的请求会被错误合并（例如 needs_js 不同拿到错误内容）。
    #[test]
    fn fingerprint_covers_all_request_affecting_fields() {
        fn fp(mutate: impl FnOnce(&mut ScrapeOptions)) -> u64 {
            let mut options = ScrapeOptions::default();
            mutate(&mut options);
            key_with("https://example.com/full-identity", &options).fingerprint()
        }

        let base = fp(|_| {});
        assert_ne!(base, fp(|o| o.method = HttpMethod::Post), "method");
        assert_ne!(base, fp(|o| o.needs_js = true), "needs_js");
        assert_ne!(base, fp(|o| o.needs_screenshot = true), "needs_screenshot");
        assert_ne!(base, fp(|o| o.mobile = true), "mobile");
        assert_ne!(base, fp(|o| o.timeout = Duration::from_secs(5)), "timeout");
        assert_ne!(base, fp(|o| o.body = Some("payload".into())), "body");
        assert_ne!(base, fp(|o| o.sync_wait_ms = 1), "sync_wait_ms");
        assert_ne!(
            base,
            fp(|o| o.actions = vec![PageAction::Wait { milliseconds: 1 }]),
            "actions"
        );
        assert_ne!(
            base,
            fp(|o| o.screenshot_config = Some(ScreenshotConfig::default())),
            "screenshot_config"
        );
        assert_ne!(
            base,
            fp(|o| o.proxy = Some("http://proxy:8080".into())),
            "proxy"
        );
        assert_ne!(
            base,
            fp(|o| o.skip_tls_verification = true),
            "skip_tls_verification"
        );
        assert_ne!(
            base,
            fp(|o| {
                o.headers.insert("X-Token".into(), "v".into());
            }),
            "headers"
        );
        assert_ne!(
            base,
            fp(|o| o.needs_tls_fingerprint = true),
            "needs_tls_fingerprint"
        );
        assert_ne!(base, fp(|o| o.use_fire_engine = true), "use_fire_engine");
        assert_ne!(base, fp(|o| o.block_ads = true), "block_ads");
        assert_ne!(base, fp(|o| o.block_media = true), "block_media");
        assert_ne!(
            base,
            fp(|o| o.session_id = Some("session-1".into())),
            "session_id"
        );
        assert_ne!(
            base,
            fp(|o| o.cache_mode = Some(CacheMode::Bypass)),
            "cache_mode"
        );
        assert_ne!(
            base,
            fp(|o| o.wait_for = Some(WaitFor::Selector("main".into()))),
            "wait_for"
        );
        assert_ne!(base, fp(|o| o.needs_mllm = true), "needs_mllm");
    }

    /// H1 行为回归：同租户同 URL 但 needs_js 不同的请求不合并
    #[test]
    fn same_tenant_different_scrape_options_do_not_coalesce() {
        let coalescer = RequestCoalescer::new();
        let team = Id::generate();
        let url = "https://example.com/needs-js-split";

        let plain = ScrapeOptions::default();
        let rendered = ScrapeOptions {
            needs_js: true,
            ..Default::default()
        };

        let key_plain = CoalesceKey::new(team, url, &plain);
        let key_rendered = CoalesceKey::new(team, url, &rendered);
        assert_ne!(key_plain, key_rendered);

        let _plain_guard = match coalescer.try_start(&key_plain, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        let _rendered_guard = match coalescer.try_start(&key_rendered, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            CoalesceResult::Wait(..) => panic!("different needs_js must not coalesce"),
        };
        assert_eq!(coalescer.in_flight_count(), 2);
    }

    /// H2 回归：陈旧 guard Drop 不得删除同键新 generation 的条目
    ///
    /// 场景：旧 guard 仍存活时条目被 purge，同键插入新 guard。旧 guard Drop
    /// 若按 key 无条件删除，会误删新条目并向新等待方广播 Completed（30473）。
    #[test]
    fn stale_guard_drop_does_not_remove_replacement_entry() {
        let coalescer = RequestCoalescer::new();
        let url = "https://example.com/generation";
        let key = key(url);

        let old_guard = match coalescer.try_start(&key, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        // 模拟 purge_stale：旧条目被清理，但旧 guard 尚未 Drop
        coalescer.purge_key(&key);
        assert!(!coalescer.is_in_flight(&key));

        // 同键新条目（新 generation）与新等待方
        let new_guard = match coalescer.try_start(&key, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        let mut rx_new = match coalescer.try_start(&key, Id::generate()) {
            CoalesceResult::Wait(rx, _) => rx,
            _ => panic!("expected Wait"),
        };

        // 旧 guard Drop：generation 不匹配，不得删除新条目/广播 Completed
        drop(old_guard);
        assert!(
            coalescer.is_in_flight(&key),
            "stale guard must not remove replacement entry"
        );
        assert_eq!(coalescer.in_flight_count(), 1);
        assert!(
            matches!(
                rx_new.try_recv(),
                Err(broadcast::error::TryRecvError::Empty)
            ),
            "new waiters must not receive stale Completed"
        );

        // 新 guard Drop 仍正常广播并清理
        drop(new_guard);
        assert!(!coalescer.is_in_flight(&key));
        assert_eq!(
            rx_new.try_recv().expect("new guard must broadcast"),
            CoalesceSignal::Completed
        );
    }
}
