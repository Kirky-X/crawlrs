// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 请求合并键
//!
//! 合并键 = 租户 + 规范化 URL + 请求身份指纹。键构造依赖 `domain::id::Id`
//! 与 `engines::types::ScrapeOptions`，指纹实现位于
//! [`crate::workers::request_identity`]；`utils::coalesce` 只保留与业务无关的
//! 泛型 single-flight 原语，避免 `utils` → `domain`/`engines` 的反向依赖（M3）。
//!
//! # 安全约束（M-2：跨租户内容泄露）
//!
//! 合并键必含租户维度。仅以 URL 为键时，租户 A 与租户 B 在等待窗口内请求
//! 同一 URL 会命中同一 in-flight 条目，等待方按 leader task_id 读到另一租户
//! 的抓取结果。租户 + 完整请求身份指纹进入键后：
//! - 不同租户永不合并（跨租户泄露路径不可达）
//! - 同租户但请求身份不同（headers/proxy/method/body/needs_js/... 任一内容
//!   相关 `ScrapeOptions` 字段）也不合并（避免带认证头或不同渲染选项的请求
//!   互相串结果）
//! - 同租户且完整请求身份相同 → 合并
//!
//! # cache_mode 与合并语义
//!
//! 缓存键的 `request_fingerprint` 把 `cache_mode` 归一为 `null`（四种模式同一键）；
//! 合并键改用 [`coalesce_fingerprint`]，在相同投影上叠加由 `cache_mode` 派生的
//! `cache_read_allowed` 布尔字段：
//! - `Enabled` / `None` / `ReadOnly`（允许读缓存）→ 同一合并键
//! - `Bypass` / `Disabled`（禁止读缓存）→ 另一合并键，两者彼此相同
//!
//! 这样 `Bypass`/`Disabled` 等待方不会收养 leader 从缓存命中得到的结果，
//! 缓存键四模式仍保持相等。
//!
//! # proxy 与合并语义（M7）
//!
//! 仅**请求级 proxy**（`ScrapeOptions::proxy`）参与合并判定：它是请求身份字段，
//! 进入指纹。`ProxyProvider` 在运行时按 round-robin/sticky 策略选出的出口代理
//! 不在 `ScrapeOptions` 中，**不参与合并键**——同一合并键的两个请求可能分别经
//! 不同出口代理抓取。需要按出口代理严格区分时，调用方应显式设置
//! `ScrapeOptions::proxy`。

use crate::domain::id::Id;
use crate::engines::types::ScrapeOptions;
use crate::utils::coalesce::{
    CoalesceGuard as SingleFlightGuard, CoalesceResult as SingleFlightResult,
    RequestCoalescer as SingleFlight,
};
use crate::workers::request_identity::coalesce_fingerprint;

/// 请求合并器（业务键 + leader 任务 ID）
pub type RequestCoalescer = SingleFlight<CoalesceKey, Id>;

/// 合并 guard（业务键 + leader 任务 ID）
pub type CoalesceGuard = SingleFlightGuard<CoalesceKey, Id>;

/// `try_start` 返回结果（业务键 + leader 任务 ID）
pub type CoalesceResult = SingleFlightResult<CoalesceKey, Id>;

/// 请求合并键：租户 + 规范化 URL + 完整请求身份指纹
///
/// 三个维度共同决定"两个请求是否可以共用一次抓取结果"：
/// - `team_id`：租户隔离边界，跨租户永不合并（M-2 修复点）
/// - `url`：规范化后的目标 URL（等价写法归一）
/// - `fingerprint`：完整请求身份的确定性摘要（`ScrapeOptions` 中与内容相关的
///   字段，含 headers 与请求级 proxy），并叠加由 `cache_mode` 派生的
///   `cache_read_allowed` 布尔投影
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
    /// - `options`：本次请求实际下发的完整选项（含 headers 与请求级 proxy）
    ///
    /// # 指纹语义
    ///
    /// - **完整**：`ScrapeOptions` 中所有影响请求/响应内容的字段都参与指纹
    ///   （method/body/needs_js/session_id/mobile/needs_screenshot/
    ///   screenshot_config/actions/wait_for/block_ads/block_media/
    ///   use_fire_engine/needs_mllm/skip_tls_verification/headers/proxy 等），
    ///   新增字段自动纳入；`cache_mode` 本身归一为 `None`，但由它派生的
    ///   `cache_read_allowed` 布尔投影参与（见 [`coalesce_fingerprint`]）
    /// - **proxy 边界**：只有请求级 `options.proxy` 参与指纹；`ProxyProvider`
    ///   运行时选路（round-robin/sticky）不入键
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
            fingerprint: coalesce_fingerprint(options),
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

    /// 请求指纹（完整请求身份 + cache_read_allowed 投影的稳定摘要，测试用）
    #[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{CacheMode, HttpMethod};
    use crate::engines::types::{PageAction, ScreenshotConfig, WaitFor};
    use std::collections::HashMap;
    use std::time::Duration;

    /// 测试固定租户：同一用例内构造的键共享租户维度
    fn test_team() -> Id {
        Id::from_u128(0x7e57_0000_0000_0000_0000_0000_0000_0001)
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

    /// 同租户 + 同 headers 但请求级 proxy 不同不合并
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
            fp(|o| o.wait_for = Some(WaitFor::Selector("main".into()))),
            "wait_for"
        );
        assert_ne!(base, fp(|o| o.needs_mllm = true), "needs_mllm");
    }

    /// H1 回归：合并键的 `cache_read_allowed` 投影
    ///
    /// `Enabled` / `None` / `ReadOnly` 允许读缓存，共用同一合并键；
    /// `Bypass` / `Disabled` 禁止读缓存，与可读模式分开（彼此相同）。
    /// 这防止 `Bypass`/`Disabled` 等待方收养 leader 的缓存命中结果。
    /// 缓存键另由 `request_fingerprint` 计算，四模式仍相等（见 cache_utils 测试）。
    #[test]
    fn merge_key_projects_cache_read_allowed() {
        fn fp(cache_mode: Option<CacheMode>) -> u64 {
            let options = ScrapeOptions {
                cache_mode,
                ..Default::default()
            };
            key_with("https://example.com/cache-mode", &options).fingerprint()
        }

        let readable = fp(Some(CacheMode::Enabled));
        assert_eq!(fp(None), readable, "None defaults to Enabled");
        assert_eq!(fp(Some(CacheMode::ReadOnly)), readable, "ReadOnly reads");

        let bypass = fp(Some(CacheMode::Bypass));
        let disabled = fp(Some(CacheMode::Disabled));
        assert_ne!(bypass, readable, "Bypass must not merge with Enabled");
        assert_ne!(disabled, readable, "Disabled must not merge with Enabled");
        assert_eq!(bypass, disabled, "both modes forbid cache reads");

        // 投影只针对 cache_mode：headers 与 needs_js 仍必须敏感
        let mut with_header = ScrapeOptions {
            cache_mode: Some(CacheMode::Bypass),
            ..Default::default()
        };
        with_header
            .headers
            .insert("Authorization".into(), "Bearer token".into());
        assert_ne!(
            readable,
            key_with("https://example.com/cache-mode", &with_header).fingerprint(),
            "headers must stay part of request identity"
        );

        let with_js = ScrapeOptions {
            cache_mode: Some(CacheMode::ReadOnly),
            needs_js: true,
            ..Default::default()
        };
        assert_ne!(
            readable,
            key_with("https://example.com/cache-mode", &with_js).fingerprint(),
            "needs_js must stay part of request identity"
        );
    }

    /// 行为回归：`ReadOnly` 等待方可与 `Enabled` leader 合并（两者都允许读缓存）
    #[test]
    fn read_only_waiter_coalesces_with_enabled_leader() {
        let coalescer = RequestCoalescer::new();
        let team = Id::generate();
        let url = "https://example.com/read-only-merge";

        let enabled = CoalesceKey::new(
            team,
            url,
            &ScrapeOptions {
                cache_mode: Some(CacheMode::Enabled),
                ..Default::default()
            },
        );
        let read_only = CoalesceKey::new(
            team,
            url,
            &ScrapeOptions {
                cache_mode: Some(CacheMode::ReadOnly),
                ..Default::default()
            },
        );
        assert_eq!(enabled, read_only);

        let _guard = match coalescer.try_start(&enabled, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        assert!(
            matches!(
                coalescer.try_start(&read_only, Id::generate()),
                CoalesceResult::Wait(..)
            ),
            "ReadOnly waiter must coalesce with Enabled leader"
        );
    }

    /// 行为回归：`Bypass` 等待方不得收养 `Enabled` leader 的（可能来自缓存的）结果
    #[test]
    fn bypass_waiter_does_not_coalesce_with_enabled_leader() {
        let coalescer = RequestCoalescer::new();
        let team = Id::generate();
        let url = "https://example.com/bypass-merge";

        let enabled = CoalesceKey::new(team, url, &ScrapeOptions::default());
        let bypass = CoalesceKey::new(
            team,
            url,
            &ScrapeOptions {
                cache_mode: Some(CacheMode::Bypass),
                ..Default::default()
            },
        );
        let disabled = CoalesceKey::new(
            team,
            url,
            &ScrapeOptions {
                cache_mode: Some(CacheMode::Disabled),
                ..Default::default()
            },
        );
        assert_ne!(enabled, bypass);
        assert_ne!(enabled, disabled);
        assert_eq!(bypass, disabled);

        let _guard = match coalescer.try_start(&enabled, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        let _bypass_guard = match coalescer.try_start(&bypass, Id::generate()) {
            CoalesceResult::Proceed(g) => g,
            CoalesceResult::Wait(..) => {
                panic!("Bypass waiter must not adopt a cache-reading leader's result")
            }
        };
        match coalescer.try_start(&disabled, Id::generate()) {
            // Disabled 与 Bypass 同属"禁止读缓存"，彼此合并（但不与 Enabled 合并）
            CoalesceResult::Wait(..) => (),
            CoalesceResult::Proceed(_) => {
                panic!("Disabled and Bypass share the same no-read merge key")
            }
        };
        assert_eq!(coalescer.in_flight_count(), 2);
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
}
