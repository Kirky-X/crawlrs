// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 请求拦截控制器（design.md §6，R-jsrender-003）
//!
//! 基于 CDP `Fetch.enable` + `Fetch.continueRequest` / `Fetch.failRequest` 实现
//! 浏览器请求拦截：
//! - [`InterceptController::block_ads`]：域名黑名单广告/追踪域名拦截（≥20 条）
//! - [`InterceptController::block_media`]：可选媒体资源拦截（image / media / font）
//!
//! # DIP 重构（H-3）
//!
//! 旧实现直接依赖 CDP `chromiumoxide::cdp::browser_protocol::network::ResourceType`，
//! 导致业务策略与底层 CDP 实现耦合，无法独立测试或复用至非 CDP 引擎。
//!
//! H-3 重构引入领域枚举 [`ResourceKind`]：
//! - [`InterceptController`] 接受 `Option<ResourceKind>`，不依赖任何 CDP 类型
//! - CDP 适配器 `From<ResourceType> for ResourceKind` 在调用方边界
//!   （`playwright.rs`）完成转换，保证领域层纯净
//!
//! # 集成位置
//!
//! 在 [`crate::engines::client::playwright::PlaywrightEngine`] 中：
//! 1. `page.execute(FetchEnableParams::default())` 启用请求暂停事件
//! 2. `page.event_listener::<EventRequestPaused>()` 获取事件流
//! 3. 对每个事件调用 [`InterceptController::should_block`] 判断是否拦截
//! 4. 命中则 `page.execute(FailRequestParams::new(..., BlockedByClient))`
//!    否则 `page.execute(ContinueRequestParams::new(...))`
//!
//! # 计数
//!
//! [`InterceptController::record_block`] 累加原子计数器，
//! [`InterceptController::intercepted_count`] 暴露当前累计拦截数。

use chromiumoxide::cdp::browser_protocol::network::{ErrorReason, ResourceType};
use std::sync::atomic::{AtomicU64, Ordering};

/// 领域层资源类型（H-3 重构：与 CDP `ResourceType` 解耦）
///
/// 纯领域枚举，描述资源语义。所有拦截策略基于此枚举判断，
/// 不依赖任何具体浏览器/CDP 实现细节。
///
/// # 与 CDP `ResourceType` 的关系
///
/// `From<ResourceType> for ResourceKind` 在调用方边界完成转换
/// （见 [`InterceptController`] 调用方 `playwright.rs`）。
/// CDP 引入新类型时只需扩展该 `From` 实现，不影响业务策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    /// HTML 文档
    Document,
    /// JavaScript 脚本
    Script,
    /// CSS 样式表
    Stylesheet,
    /// 图片
    Image,
    /// 音频/视频
    Media,
    /// 字体
    Font,
    /// XMLHttpRequest
    Xhr,
    /// Fetch API 请求
    Fetch,
    /// 其他未分类类型
    Other,
}

/// CDP `ResourceType` → 领域 `ResourceKind` 适配器
///
/// 在调用方边界完成转换，将外部 CDP 类型映射到内部领域类型。
/// 未知/新增 CDP 类型映射为 [`ResourceKind::Other`]，保证前向兼容。
impl From<ResourceType> for ResourceKind {
    fn from(rt: ResourceType) -> Self {
        match rt {
            ResourceType::Document => Self::Document,
            ResourceType::Script => Self::Script,
            ResourceType::Stylesheet => Self::Stylesheet,
            ResourceType::Image => Self::Image,
            ResourceType::Media => Self::Media,
            ResourceType::Font => Self::Font,
            ResourceType::Xhr => Self::Xhr,
            ResourceType::Fetch => Self::Fetch,
            _ => Self::Other,
        }
    }
}

/// 广告/追踪域名黑名单（R-jsrender-003）
///
/// 覆盖主流广告网络、追踪 SDK、用户分析平台。
/// 命中任一子串即视为广告/追踪请求并 abort。
/// 至少 20 条（任务要求），实际维护 30+ 条。
pub const AD_DOMAIN_BLACKLIST: &[&str] = &[
    // Google Advertising
    "doubleclick.net",
    "googlesyndication.com",
    "googleadservices.com",
    "googletagmanager.com",
    "googletagservices.com",
    "adservice.google.com",
    "adsense.com",
    // Google Analytics
    "google-analytics.com",
    "ssl.google-analytics.com",
    // Facebook
    "facebook.com/tr",
    "facebook.net",
    "connect.facebook.net",
    // Hotjar
    "hotjar.com",
    // Quantcast
    "quantserve.com",
    // Scorecard Research (comScore)
    "scorecardresearch.com",
    // Major Ad Exchanges
    "adnxs.com",           // AppNexus / Xandr
    "taboola.com",
    "outbrain.com",
    "criteo.com",
    "rubiconproject.com",
    "pubmatic.com",
    "openx.net",
    "casalemedia.com",     // Index Exchange
    "agkn.com",
    "bluekai.com",         // Oracle Data Cloud
    "adform.net",
    "yieldmo.com",
    "moatads.com",
    "admixer.net",
    // Twitter
    "ads-twitter.com",
    "analytics.twitter.com",
    // LinkedIn
    "px.ads.linkedin.com",
    // TikTok
    "analytics.tiktok.com",
    // Microsoft / Bing Ads
    "bat.bing.com",
    // Amazon Ads
    "amazon-adsystem.com",
];

/// 媒体资源种类集合（当 `block_media = true` 时拦截）
///
/// H-3 重构后基于领域枚举 [`ResourceKind`]，与 CDP 解耦。
/// 拦截策略：Image / Media / Font。
pub const MEDIA_RESOURCE_KINDS: &[ResourceKind] = &[
    ResourceKind::Image,
    ResourceKind::Media,
    ResourceKind::Font,
];

/// 请求拦截控制器（design.md §6，R-jsrender-003）
///
/// # 字段
///
/// - `block_ads`：是否启用广告/追踪域名黑名单拦截
/// - `block_media`：是否启用媒体资源种类拦截（image/media/font）
/// - `intercepted_count`：累计拦截请求数（原子计数，线程安全）
///
/// # 用法
///
/// ```no_run
/// # use crawlrs::engines::intercept::{InterceptController, ResourceKind};
/// let ctrl = InterceptController::new(true, false);
/// assert!(ctrl.should_block_ad_domain("https://doubleclick.net/pixel.gif"));
/// assert!(!ctrl.should_block_ad_domain("https://example.com/index.html"));
/// ctrl.record_block();
/// assert_eq!(ctrl.intercepted_count(), 1);
/// ```
#[derive(Debug)]
pub struct InterceptController {
    /// 是否拦截广告/追踪域名
    pub block_ads: bool,
    /// 是否拦截媒体资源（image/media/font）
    pub block_media: bool,
    /// 累计拦截请求数（原子计数）
    intercepted_count: AtomicU64,
}

impl InterceptController {
    /// 创建新控制器
    ///
    /// # 参数
    ///
    /// - `block_ads`：true 则命中 [`AD_DOMAIN_BLACKLIST`] 的请求被拦截
    /// - `block_media`：true 则媒体资源种类被拦截
    #[must_use]
    pub fn new(block_ads: bool, block_media: bool) -> Self {
        Self {
            block_ads,
            block_media,
            intercepted_count: AtomicU64::new(0),
        }
    }

    /// 默认关闭拦截（兼容旧行为）
    #[must_use]
    pub fn disabled() -> Self {
        Self::new(false, false)
    }

    /// 判断 URL 是否应被拦截（R-jsrender-003）
    ///
    /// 综合判断：
    /// 1. 如果 `block_ads` 且 URL 命中黑名单 → true
    /// 2. 如果 `block_media` 且资源种类属于媒体 → true
    /// 3. 否则 false
    ///
    /// # 参数
    ///
    /// - `url`：请求 URL（完整 URL 或 path）
    /// - `kind`：领域资源种类（H-3 重构：原 CDP `&ResourceType`，现 `ResourceKind`）
    ///   可为 None，则跳过媒体判断
    #[must_use]
    pub fn should_block(&self, url: &str, kind: Option<ResourceKind>) -> bool {
        if self.block_ads && self.should_block_ad_domain(url) {
            return true;
        }
        if self.block_media && self.should_block_media(kind) {
            return true;
        }
        false
    }

    /// 判断 URL 是否命中广告/追踪域名黑名单
    ///
    /// 大小写不敏感，子串匹配（覆盖子域名 + 路径）。
    #[must_use]
    pub fn should_block_ad_domain(&self, url: &str) -> bool {
        if !self.block_ads {
            return false;
        }
        let lower = url.to_ascii_lowercase();
        AD_DOMAIN_BLACKLIST.iter().any(|d| lower.contains(d))
    }

    /// 判断资源种类是否属于媒体（image/media/font）
    ///
    /// H-3 重构：入参由 `Option<&ResourceType>` 改为 `Option<ResourceKind>`，
    /// 与 CDP 实现解耦。
    #[must_use]
    pub fn should_block_media(&self, kind: Option<ResourceKind>) -> bool {
        if !self.block_media {
            return false;
        }
        match kind {
            Some(k) => MEDIA_RESOURCE_KINDS.contains(&k),
            None => false,
        }
    }

    /// 记录一次拦截（原子递增）
    pub fn record_block(&self) {
        self.intercepted_count.fetch_add(1, Ordering::Relaxed);
    }

    /// 获取累计拦截请求数
    #[must_use]
    pub fn intercepted_count(&self) -> u64 {
        self.intercepted_count.load(Ordering::Relaxed)
    }
}

impl Default for InterceptController {
    fn default() -> Self {
        Self::disabled()
    }
}

impl Clone for InterceptController {
    /// 克隆时保留 `block_ads` / `block_media` 配置，**重置计数为 0**。
    ///
    /// 计数器是运行时状态，克隆出的新控制器通常用于新的页面会话，
    /// 不应继承原控制器的累计计数。
    fn clone(&self) -> Self {
        Self::new(self.block_ads, self.block_media)
    }
}

/// 拦截请求时使用的错误原因（CDP `Fetch.failRequest` 的 errorReason 参数）
///
/// `BlockedByClient` 是广告拦截扩展（uBlock Origin / AdBlock）的标准行为，
/// 站点侧通常对此原因码有兼容处理（不会触发重试或错误页）。
pub const BLOCK_REASON: ErrorReason = ErrorReason::BlockedByClient;

#[cfg(test)]
mod tests {
    use super::*;

    // === AD_DOMAIN_BLACKLIST 完整性 ===

    #[test]
    fn ad_domain_blacklist_has_at_least_20_entries() {
        // 任务要求 ≥20 条
        assert!(
            AD_DOMAIN_BLACKLIST.len() >= 20,
            "AD_DOMAIN_BLACKLIST must have >= 20 entries, got {}",
            AD_DOMAIN_BLACKLIST.len()
        );
    }

    #[test]
    fn ad_domain_blacklist_contains_required_entries() {
        // 任务清单要求的代表性域名
        let required = [
            "doubleclick.net",
            "google-analytics.com",
            "facebook.com/tr",
            "googletagmanager.com",
            "hotjar.com",
            "quantserve.com",
            "scorecardresearch.com",
        ];
        for r in &required {
            assert!(
                AD_DOMAIN_BLACKLIST.iter().any(|d| d == r),
                "AD_DOMAIN_BLACKLIST missing required entry: {}",
                r
            );
        }
    }

    #[test]
    fn ad_domain_blacklist_entries_are_lowercase() {
        // 子串匹配大小写不敏感，但黑名单本身保持小写便于审计
        for d in AD_DOMAIN_BLACKLIST {
            assert!(
                d.chars().all(|c| !c.is_ascii_uppercase()),
                "AD_DOMAIN_BLACKLIST entry must be lowercase: {}",
                d
            );
        }
    }

    #[test]
    fn ad_domain_blacklist_no_duplicates() {
        let mut sorted = AD_DOMAIN_BLACKLIST.to_vec();
        sorted.sort();
        let dedup_count = sorted.windows(2).filter(|w| w[0] == w[1]).count();
        assert_eq!(dedup_count, 0, "AD_DOMAIN_BLACKLIST has duplicates");
    }

    // === should_block_ad_domain ===

    #[test]
    fn should_block_ad_domain_doubleclick() {
        let ctrl = InterceptController::new(true, false);
        assert!(ctrl.should_block_ad_domain("https://doubleclick.net/pixel.gif"));
        assert!(ctrl.should_block_ad_domain("https://ad.doubleclick.net/ddm/track"));
    }

    #[test]
    fn should_block_ad_domain_google_analytics() {
        let ctrl = InterceptController::new(true, false);
        assert!(ctrl.should_block_ad_domain("https://google-analytics.com/collect"));
        assert!(ctrl.should_block_ad_domain("https://ssl.google-analytics.com/g.js"));
    }

    #[test]
    fn should_block_ad_domain_facebook_tracker() {
        let ctrl = InterceptController::new(true, false);
        // facebook.com/tr 是追踪端点，但 facebook.com 本身是社交网站
        // 我们的策略是子串匹配，会命中 facebook.com/tr
        assert!(ctrl.should_block_ad_domain("https://facebook.com/tr?id=123"));
    }

    #[test]
    fn should_block_ad_domain_normal_url_passes() {
        let ctrl = InterceptController::new(true, false);
        assert!(!ctrl.should_block_ad_domain("https://example.com/index.html"));
        assert!(!ctrl.should_block_ad_domain("https://www.wikipedia.org/wiki/Main_Page"));
        assert!(!ctrl.should_block_ad_domain("https://github.com/rust-lang/rust"));
    }

    #[test]
    fn should_block_ad_domain_case_insensitive() {
        let ctrl = InterceptController::new(true, false);
        assert!(ctrl.should_block_ad_domain("HTTPS://DOUBLECLICK.NET/X"));
        assert!(ctrl.should_block_ad_domain("https://Google-Analytics.com/collect"));
    }

    #[test]
    fn should_block_ad_domain_disabled_passes_everything() {
        // block_ads=false 时所有 URL 都放行
        let ctrl = InterceptController::new(false, false);
        assert!(!ctrl.should_block_ad_domain("https://doubleclick.net/pixel.gif"));
        assert!(!ctrl.should_block_ad_domain("https://google-analytics.com/collect"));
    }

    #[test]
    fn should_block_ad_domain_empty_url_passes() {
        let ctrl = InterceptController::new(true, false);
        assert!(!ctrl.should_block_ad_domain(""));
    }

    // === should_block_media ===

    #[test]
    fn should_block_media_image_when_enabled() {
        let ctrl = InterceptController::new(false, true);
        assert!(ctrl.should_block_media(Some(ResourceKind::Image)));
    }

    #[test]
    fn should_block_media_media_when_enabled() {
        let ctrl = InterceptController::new(false, true);
        assert!(ctrl.should_block_media(Some(ResourceKind::Media)));
    }

    #[test]
    fn should_block_media_font_when_enabled() {
        let ctrl = InterceptController::new(false, true);
        assert!(ctrl.should_block_media(Some(ResourceKind::Font)));
    }

    #[test]
    fn should_block_media_disabled_passes_all_resource_types() {
        let ctrl = InterceptController::new(false, false);
        assert!(!ctrl.should_block_media(Some(ResourceKind::Image)));
        assert!(!ctrl.should_block_media(Some(ResourceKind::Media)));
        assert!(!ctrl.should_block_media(Some(ResourceKind::Font)));
    }

    #[test]
    fn should_block_media_passes_non_media_resource_types() {
        // block_media=true 但资源类型不是 image/media/font，应放行
        let ctrl = InterceptController::new(false, true);
        assert!(!ctrl.should_block_media(Some(ResourceKind::Document)));
        assert!(!ctrl.should_block_media(Some(ResourceKind::Script)));
        assert!(!ctrl.should_block_media(Some(ResourceKind::Stylesheet)));
        assert!(!ctrl.should_block_media(Some(ResourceKind::Xhr)));
        assert!(!ctrl.should_block_media(Some(ResourceKind::Fetch)));
    }

    #[test]
    fn should_block_media_none_resource_type_returns_false() {
        // resource_type=None 时即使 block_media=true 也不拦截
        // 因为无法判断资源类型
        let ctrl = InterceptController::new(false, true);
        assert!(!ctrl.should_block_media(None));
    }

    // === should_block（综合判断）===

    #[test]
    fn should_block_ads_blacklist_hit_returns_true() {
        let ctrl = InterceptController::new(true, false);
        assert!(ctrl.should_block("https://doubleclick.net/x", None));
        assert!(ctrl.should_block(
            "https://google-analytics.com/collect",
            Some(ResourceKind::Xhr)
        ));
    }

    #[test]
    fn should_block_normal_url_passes() {
        let ctrl = InterceptController::new(true, false);
        assert!(!ctrl.should_block("https://example.com/index.html", None));
        assert!(!ctrl.should_block(
            "https://example.com/image.png",
            Some(ResourceKind::Image)
        ));
    }

    #[test]
    fn should_block_media_image_request_when_block_media() {
        let ctrl = InterceptController::new(false, true);
        assert!(ctrl.should_block(
            "https://example.com/image.png",
            Some(ResourceKind::Image)
        ));
    }

    #[test]
    fn should_block_media_disabled_passes_image() {
        let ctrl = InterceptController::new(false, false);
        assert!(!ctrl.should_block(
            "https://example.com/image.png",
            Some(ResourceKind::Image)
        ));
    }

    #[test]
    fn should_block_both_ad_and_media_hit_returns_true() {
        // 同时启用两个开关，命中任一即拦截
        let ctrl = InterceptController::new(true, true);
        // 广告命中
        assert!(ctrl.should_block("https://doubleclick.net/x", None));
        // 媒体命中（URL 不在黑名单但资源类型是 Image）
        assert!(ctrl.should_block(
            "https://example.com/banner.png",
            Some(ResourceKind::Image)
        ));
    }

    #[test]
    fn should_block_disabled_passes_everything() {
        let ctrl = InterceptController::new(false, false);
        assert!(!ctrl.should_block("https://doubleclick.net/x", None));
        assert!(!ctrl.should_block(
            "https://example.com/image.png",
            Some(ResourceKind::Image)
        ));
    }

    #[test]
    fn should_block_ad_url_with_media_type_returns_true() {
        // 广告 URL + Image 资源类型：block_ads=true → 拦截
        // 即使 block_media=false，因为 ad 命中
        let ctrl = InterceptController::new(true, false);
        assert!(ctrl.should_block(
            "https://doubleclick.net/banner.png",
            Some(ResourceKind::Image)
        ));
    }

    // === record_block / intercepted_count ===

    #[test]
    fn record_block_increments_count() {
        let ctrl = InterceptController::new(true, false);
        assert_eq!(ctrl.intercepted_count(), 0);
        ctrl.record_block();
        ctrl.record_block();
        ctrl.record_block();
        assert_eq!(ctrl.intercepted_count(), 3);
    }

    #[test]
    fn intercepted_count_starts_at_zero() {
        let ctrl = InterceptController::new(true, true);
        assert_eq!(ctrl.intercepted_count(), 0);
    }

    #[test]
    fn record_block_concurrent_safe() {
        // 并发场景验证（粗略）：多线程同时 record_block，最终计数等于线程数*迭代数
        let ctrl = std::sync::Arc::new(InterceptController::new(true, false));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let c = std::sync::Arc::clone(&ctrl);
            handles.push(std::thread::spawn(move || {
                for _ in 0..100 {
                    c.record_block();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(ctrl.intercepted_count(), 400);
    }

    // === new / default / clone ===

    #[test]
    fn new_with_both_disabled() {
        let ctrl = InterceptController::new(false, false);
        assert!(!ctrl.block_ads);
        assert!(!ctrl.block_media);
        assert_eq!(ctrl.intercepted_count(), 0);
    }

    #[test]
    fn new_with_both_enabled() {
        let ctrl = InterceptController::new(true, true);
        assert!(ctrl.block_ads);
        assert!(ctrl.block_media);
    }

    #[test]
    fn disabled_equals_new_false_false() {
        let a = InterceptController::disabled();
        let b = InterceptController::new(false, false);
        assert_eq!(a.block_ads, b.block_ads);
        assert_eq!(a.block_media, b.block_media);
    }

    #[test]
    fn default_is_disabled() {
        let ctrl = InterceptController::default();
        assert!(!ctrl.block_ads);
        assert!(!ctrl.block_media);
    }

    #[test]
    fn clone_preserves_config_but_resets_count() {
        // Clone 保留 block_ads / block_media 配置，但重置计数
        let ctrl = InterceptController::new(true, true);
        ctrl.record_block();
        ctrl.record_block();
        assert_eq!(ctrl.intercepted_count(), 2);
        let cloned = ctrl.clone();
        assert!(cloned.block_ads);
        assert!(cloned.block_media);
        assert_eq!(cloned.intercepted_count(), 0, "cloned controller must reset count");
    }

    // === BLOCK_REASON ===

    #[test]
    fn block_reason_is_blocked_by_client() {
        // uBlock / AdBlock 兼容码
        assert_eq!(BLOCK_REASON, ErrorReason::BlockedByClient);
    }

    // === MEDIA_RESOURCE_KINDS ===

    #[test]
    fn media_resource_kinds_contains_image_media_font() {
        assert!(MEDIA_RESOURCE_KINDS.contains(&ResourceKind::Image));
        assert!(MEDIA_RESOURCE_KINDS.contains(&ResourceKind::Media));
        assert!(MEDIA_RESOURCE_KINDS.contains(&ResourceKind::Font));
        assert_eq!(MEDIA_RESOURCE_KINDS.len(), 3);
    }

    // === 端到端：典型广告请求场景 ===

    #[test]
    fn end_to_end_doubleclick_pixel_blocked_and_counted() {
        let ctrl = InterceptController::new(true, false);
        let url = "https://stats.g.doubleclick.net/r/collect?v=1&aip=1";
        let should = ctrl.should_block(url, Some(ResourceKind::Image));
        assert!(should, "doubleclick pixel must be blocked");
        if should {
            ctrl.record_block();
        }
        assert_eq!(ctrl.intercepted_count(), 1);
    }

    #[test]
    fn end_to_end_google_analytics_xhr_blocked() {
        let ctrl = InterceptController::new(true, false);
        let url = "https://www.google-analytics.com/analytics.js";
        let should = ctrl.should_block(url, Some(ResourceKind::Script));
        assert!(should);
    }

    #[test]
    fn end_to_end_facebook_pixel_blocked() {
        let ctrl = InterceptController::new(true, false);
        let url = "https://connect.facebook.net/en_US/fbevents.js";
        let should = ctrl.should_block(url, Some(ResourceKind::Script));
        assert!(should);
    }

    #[test]
    fn end_to_end_normal_page_resources_pass() {
        let ctrl = InterceptController::new(true, true);
        // 主文档放行
        assert!(!ctrl.should_block(
            "https://example.com/index.html",
            Some(ResourceKind::Document)
        ));
        // 业务脚本放行
        assert!(!ctrl.should_block(
            "https://example.com/app.js",
            Some(ResourceKind::Script)
        ));
        // 业务 XHR 放行
        assert!(!ctrl.should_block(
            "https://api.example.com/v1/users",
            Some(ResourceKind::Xhr)
        ));
    }

    #[test]
    fn end_to_end_business_image_blocked_when_block_media() {
        // 业务站点的图片在 block_media=true 时也会被拦截
        // 这是预期的代价：减少带宽，但可能影响内容抓取
        // 用户应根据抓取目标决定是否启用 block_media
        let ctrl = InterceptController::new(false, true);
        assert!(ctrl.should_block(
            "https://example.com/product.png",
            Some(ResourceKind::Image)
        ));
    }

    // === H-3: From<ResourceType> for ResourceKind 适配器测试 ===

    #[test]
    fn from_resource_type_maps_known_variants() {
        // 已知 CDP ResourceType 必须无损映射到对应领域变体
        assert_eq!(ResourceKind::from(ResourceType::Document), ResourceKind::Document);
        assert_eq!(ResourceKind::from(ResourceType::Script), ResourceKind::Script);
        assert_eq!(ResourceKind::from(ResourceType::Stylesheet), ResourceKind::Stylesheet);
        assert_eq!(ResourceKind::from(ResourceType::Image), ResourceKind::Image);
        assert_eq!(ResourceKind::from(ResourceType::Media), ResourceKind::Media);
        assert_eq!(ResourceKind::from(ResourceType::Font), ResourceKind::Font);
        assert_eq!(ResourceKind::from(ResourceType::Xhr), ResourceKind::Xhr);
        assert_eq!(ResourceKind::from(ResourceType::Fetch), ResourceKind::Fetch);
    }

    #[test]
    fn from_resource_type_unknown_maps_to_other() {
        // CDP 引入未识别的新类型时映射为 Other，保证前向兼容
        // 使用 WebSocket 等非标准 ResourceType 进行验证（CDP 0.9.1 已有此变体）
        let mapped = ResourceKind::from(ResourceType::WebSocket);
        assert_eq!(mapped, ResourceKind::Other);
    }

    #[test]
    fn from_resource_type_can_be_used_in_should_block() {
        // 端到端：调用方（playwright.rs）在边界处 from(resource_type) 转换后传入
        let ctrl = InterceptController::new(false, true);
        let cdp_image: ResourceType = ResourceType::Image;
        let kind = ResourceKind::from(cdp_image);
        assert!(ctrl.should_block_media(Some(kind)));
    }
}
