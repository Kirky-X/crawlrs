// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! UA 客户端提示 metadata 构造（Sec-CH-UA 一致性）
//!
//! `page.set_user_agent(profile.ua)` 只覆盖 HTTP `User-Agent` 头，而
//! `Sec-CH-UA*` 请求头与 `navigator.userAgentData` 仍由浏览器默认品牌
//! （HeadlessChrome）填充——服务端单请求即可识破 "UA 声称 Chrome、
//! 客户端提示声称 HeadlessChrome" 的矛盾。CDP
//! `Emulation.setUserAgentOverride` 的 `userAgentMetadata` 字段
//! （"To be sent in Sec-CH-UA-* headers and returned in
//! navigator.userAgentData"）是官方修正通道。
//!
//! 两个 CDP 字段的取值词表不同，**不得同源**：
//! - `metadata.platform`：Sec-CH-UA-Platform 词表（`Windows`/`macOS`/
//!   `Android`/`iOS`/`Linux`）——取 `UaProfile.platform`（逐 profile
//!   权威标签，macOS 不写 "Mac OS X"）；
//! - `SetUserAgentOverrideParams.platform`：`navigator.platform` 词表
//!   （`Win32`/`MacIntel`/`iPhone`/`iPad`/`Linux armv8l`/`Linux x86_64`）
//!   ——由 UA 串 OS 段映射。
//!
//! brands/fullVersionList 取 `UaProfile.sec_ch_ua`（逐版本权威模板，含
//! GREASE 与 Edge/Samsung 正确品牌，与 wreq 引擎发头同源）——从 UA 串重
//! 解析会把 Edge profile 解析成 "Google Chrome" 品牌，与 UA 的 `Edg/`
//! 标记自相矛盾。UA 串仅用于派生 sec_ch_ua 不携带的高熵字段
//! （platformVersion/model）。
//!
//! `sec_ch_ua` 为空（Firefox/Safari 等无 Client Hints 家族；WKWebView
//! 家族如 CriOS 已由 `UaPool::pick_chromium` 从浏览器/仿真引擎候选排除，
//! 此处 None 为纵深防御）时 `metadata` 为 None：调用方跳过 metadata
//! 设置，回退纯 UA 字符串覆盖。**绝不发送 brands 为空的 metadata**——
//! 空品牌列表是真实浏览器不可能出现的状态，比默认 HeadlessChrome 更可疑。

use chromiumoxide::cdp::browser_protocol::emulation::{UserAgentBrandVersion, UserAgentMetadata};

/// navigator.platform 覆盖值 + client hints metadata 的成对产物。
///
/// 两个字段词表不同（见模块文档），由同一 profile 派生但各自映射。
#[derive(Debug, Clone, PartialEq)]
pub struct UaFingerprint {
    /// None → 调用方跳过 `user_agent_metadata`（纯 UA 字符串覆盖）
    pub metadata: Option<UserAgentMetadata>,
    /// `SetUserAgentOverrideParams.platform`（navigator.platform 词表）；
    /// None → 不覆盖，保留浏览器默认值
    pub navigator_platform: Option<&'static str>,
}

/// architecture 固定值（UA 串不含指令集信息；Android 真机为 arm，
/// 桌面按 design 固定 x86）。
const ARCH_DESKTOP: &str = "x86";
const ARCH_ANDROID: &str = "arm";
/// bitness 固定 64（移动端 iOS Chrome 上报同样为 64 位语义）
const BITNESS: &str = "64";

/// 由 profile 构造完整的 UA 指纹覆盖（navigator.platform + client hints）。
///
/// # 参数
///
/// * `ua` - UA 字符串（`UaProfile.ua`，用于派生 sec_ch_ua 不含的高熵字段）
/// * `sec_ch_ua` - `UaProfile.sec_ch_ua` 模板（逐版本权威品牌表；空串 =
///   无 Client Hints 能力的浏览器家族）
/// * `profile_platform` - `UaProfile.platform` 标签（Sec-CH-UA-Platform 词表）
/// * `mobile` - profile 的移动端标志（与 viewport/mobile 覆盖一致）
#[must_use]
pub fn build_ua_fingerprint(
    ua: &str,
    sec_ch_ua: &str,
    profile_platform: &str,
    mobile: bool,
) -> UaFingerprint {
    let brands = parse_sec_ch_ua(sec_ch_ua);
    // brands 不可构造 → metadata 整体缺席（绝不发空品牌）；navigator.platform
    // 与 client hints 无耦合，仍可安全覆盖
    let metadata = brands.map(|b| {
        let (platform, platform_version) = client_hint_platform(profile_platform, ua);
        // Android 真机为 arm 指令集；判定须在 platform move 前完成
        let architecture = if mobile && platform == "Android" {
            ARCH_ANDROID.to_string()
        } else {
            ARCH_DESKTOP.to_string()
        };
        UserAgentMetadata {
            brands: Some(b),
            // Sec-CH-UA-Full-Version-List 仅在站点经 Accept-CH 主动索取时
            // 发送；profile 模板不含完整版本数据，伪造（如给 Edge 填 Chrome
            // 完整版本）比缺席更危险——恒缺席，缺席是正常状态
            full_version_list: None,
            platform,
            platform_version,
            architecture,
            model: parse_device_model(ua, mobile),
            mobile,
            bitness: Some(BITNESS.to_string()),
            wow64: None,
            form_factors: None,
        }
    });
    UaFingerprint {
        metadata,
        navigator_platform: navigator_platform(ua),
    }
}

/// 解析 sec_ch_ua 模板（`"Brand";v="ver", ...` header 形态）为品牌列表。
///
/// 空串（Firefox/Safari/CriOS profile）→ None。解析容错：段内缺引号/缺
/// `v=` 的残缺段跳过（模板是编译期常量，此为防御性兜底）。
fn parse_sec_ch_ua(sec_ch_ua: &str) -> Option<Vec<UserAgentBrandVersion>> {
    let trimmed = sec_ch_ua.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut brands = Vec::new();
    for part in trimmed.split(',') {
        let part = part.trim();
        let brand = part
            .strip_prefix('"')
            .and_then(|rest| rest.split("\"").next())
            .unwrap_or_default();
        if brand.is_empty() {
            continue;
        }
        let Some(v_pos) = part.find("v=\"") else {
            continue;
        };
        let version: String = part[v_pos + 3..]
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if version.is_empty() {
            continue;
        }
        brands.push(UserAgentBrandVersion::new(brand, version));
    }
    if brands.is_empty() {
        None
    } else {
        Some(brands)
    }
}

/// client hints 词表的 platform + platformVersion。
///
/// platform 优先取 profile 标签（权威）；标签缺失时按 UA OS 段以
/// client-hints 词表兜底（Macintosh → `macOS`，非 UA 原文 `Mac OS X`）。
fn client_hint_platform(profile_platform: &str, ua: &str) -> (String, String) {
    let platform = if profile_platform.is_empty() {
        ua_fallback_platform(ua).to_string()
    } else {
        profile_platform.to_string()
    };
    (platform, platform_version_from_ua(ua))
}

/// navigator.platform 词表映射（由 UA OS 段判定）。
///
/// 真实取值：Windows → `Win32`（64 位亦然）、macOS → `MacIntel`、
/// iPhone/iPad → `iPhone`/`iPad`、Android → `Linux armv8l`、
/// Linux 桌面 → `Linux x86_64`。无法判定 → None（保留浏览器默认）。
fn navigator_platform(ua: &str) -> Option<&'static str> {
    if ua.contains("iPhone") {
        Some("iPhone")
    } else if ua.contains("iPad") {
        Some("iPad")
    } else if ua.contains("Windows NT") {
        Some("Win32")
    } else if ua.contains("Macintosh") {
        Some("MacIntel")
    } else if ua.contains("Android") {
        Some("Linux armv8l")
    } else if ua.contains("Linux") {
        Some("Linux x86_64")
    } else {
        None
    }
}

/// client-hints 词表的 OS 兜底判定（profile 标签缺失时用）。
fn ua_fallback_platform(ua: &str) -> &'static str {
    if ua.contains("iPhone") || ua.contains("iPad") {
        "iOS"
    } else if ua.contains("Android") {
        "Android"
    } else if ua.contains("Macintosh") {
        "macOS"
    } else if ua.contains("Windows NT") {
        "Windows"
    } else if ua.contains("Linux") {
        "Linux"
    } else {
        ""
    }
}

/// 从 UA 派生 Sec-CH-UA-Platform-Version（`x.y.z` 段数归一）。
///
/// 判定顺序敏感：iOS UA 含 "like Mac OS X"、Android UA 含 "Linux"。
/// Linux 的 platformVersion 语义是内核版本，UA 串（"X11; Linux x86_64"）
/// 不可解析——置空而非猜测。Windows UA 冻结在 NT 10.0，归一为 `10.0.0`
/// 与之自洽（真实 Win11 Chrome 亦上报 `10.0.0` 系）。
fn platform_version_from_ua(ua: &str) -> String {
    if ua.contains("iPhone") || ua.contains("iPad") {
        let version = find_token(ua, "iPhone OS ").or_else(|| find_token(ua, "CPU OS "));
        normalize_version(version.as_deref())
    } else if let Some(raw) = find_token(ua, "Android ") {
        normalize_version(Some(&raw))
    } else if let Some(raw) = find_token(ua, "Windows NT ") {
        normalize_version(Some(&raw))
    } else if ua.contains("Macintosh") {
        normalize_version(find_token(ua, "Mac OS X ").as_deref())
    } else {
        String::new()
    }
}

/// Android 设备型号：`Android <ver>; <model>` 段（终止于 `)`；` Build/`
/// 后缀剥离）。非移动 UA → 空串（桌面 Chrome 的 Sec-CH-UA-Model 为空）。
fn parse_device_model(ua: &str, mobile: bool) -> String {
    if !mobile {
        return String::new();
    }
    let Some(rest) = ua.split_once("Android ").map(|(_, r)| r) else {
        return String::new();
    };
    let Some(after_version) = rest.split_once("; ") else {
        return String::new();
    };
    let model: String = after_version.1.chars().take_while(|c| *c != ')').collect();
    let model = model
        .split(" Build")
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    // "Android 6.0; wv" 的 wv 是 WebView 标记，非设备型号
    if model == "wv" {
        String::new()
    } else {
        model
    }
}

/// 取 UA 串中 `marker` 之后到词界（空白/分号/右括号）的 token。
fn find_token(ua: &str, marker: &str) -> Option<String> {
    let start = ua.find(marker)? + marker.len();
    let token: String = ua[start..]
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != ';' && *c != ')')
        .collect();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// 版本归一化：`10_15_7`/`10.15.7` → `10.15.7`，`10.0` → `10.0.0`，
/// `14` → `14.0.0`（对齐 Chrome Sec-CH-UA-Platform-Version 的 `x.y.z` 段数）。
///
/// 非纯数字段（内核版本串等不可解析内容）→ 空串。
fn normalize_version(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return String::new();
    };
    let normalized = raw.replace('_', ".");
    let mut parts: Vec<&str> = normalized.split('.').collect();
    if parts.len() > 3 {
        parts.truncate(3);
    }
    if parts
        .iter()
        .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return String::new();
    }
    while parts.len() < 3 {
        parts.push("0");
    }
    parts.join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ua_pool 的真实模板形态（与 SEC_CHROME_131 / SEC_EDGE_131 同构）
    const SEC_CHROME_131: &str =
        "\"Chromium\";v=\"131\", \"Not_A Brand\";v=\"24\", \"Google Chrome\";v=\"131\"";
    const SEC_EDGE_131: &str =
        "\"Microsoft Edge\";v=\"131\", \"Chromium\";v=\"131\", \"Not_A Brand\";v=\"24\", \"Google Chrome\";v=\"131\"";
    const UA_WIN_CHROME: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
    const UA_MAC_CHROME: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
    const UA_ANDROID_PIXEL: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36";
    const UA_IPHONE_CRIOS: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/131.0.6778.73 Mobile/15E148 Safari/604.1";

    fn brand_version(brands: &[UserAgentBrandVersion], brand: &str) -> Option<String> {
        brands
            .iter()
            .find(|b| b.brand == brand)
            .map(|b| b.version.clone())
    }

    #[test]
    fn test_windows_desktop_full_fingerprint() {
        let fp = build_ua_fingerprint(UA_WIN_CHROME, SEC_CHROME_131, "Windows", false);

        let meta = fp.metadata.expect("chrome profile must have metadata");
        assert_eq!(meta.platform, "Windows");
        assert_eq!(meta.platform_version, "10.0.0");
        assert_eq!(meta.architecture, "x86");
        assert_eq!(meta.bitness.as_deref(), Some("64"));
        assert_eq!(meta.model, "");
        assert!(!meta.mobile);
        let brands = meta.brands.expect("brands from sec_ch_ua template");
        assert_eq!(
            brand_version(&brands, "Google Chrome").as_deref(),
            Some("131")
        );
        assert_eq!(brand_version(&brands, "Not_A Brand").as_deref(), Some("24"));
        // navigator.platform 词表：Windows 恒为 Win32（64 位亦然）
        assert_eq!(fp.navigator_platform, Some("Win32"));
    }

    #[test]
    fn test_macos_platform_vocabularies_split() {
        let fp = build_ua_fingerprint(UA_MAC_CHROME, SEC_CHROME_131, "macOS", false);

        let meta = fp.metadata.expect("metadata present");
        // client-hints 词表：macOS（非 UA 原文 "Mac OS X"）
        assert_eq!(meta.platform, "macOS");
        assert_eq!(meta.platform_version, "10.15.7");
        // navigator.platform 词表：MacIntel
        assert_eq!(fp.navigator_platform, Some("MacIntel"));
    }

    #[test]
    fn test_edge_brands_from_template_no_google_chrome_substitution() {
        // Edge profile：brands 必须来自 sec_ch_ua 模板（含 Microsoft Edge），
        // 不得从 UA 串解析成 Google Chrome 品牌与 Edg/ 标记矛盾
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0";
        let fp = build_ua_fingerprint(ua, SEC_EDGE_131, "Windows", false);

        let meta = fp.metadata.expect("metadata present");
        let brands = meta.brands.expect("brands from Edge template");
        assert_eq!(
            brand_version(&brands, "Microsoft Edge").as_deref(),
            Some("131")
        );
        assert_eq!(
            brand_version(&brands, "Google Chrome").as_deref(),
            Some("131")
        );
        assert_eq!(fp.navigator_platform, Some("Win32"));
    }

    #[test]
    fn test_android_arm_and_device_model() {
        let fp = build_ua_fingerprint(UA_ANDROID_PIXEL, SEC_CHROME_131, "Android", true);

        let meta = fp.metadata.expect("metadata present");
        assert_eq!(meta.platform, "Android");
        assert_eq!(meta.platform_version, "14.0.0");
        assert_eq!(meta.architecture, "arm");
        assert_eq!(meta.model, "Pixel 8");
        assert!(meta.mobile);
        // navigator.platform 词表：Android Chrome 报 Linux armv8l
        assert_eq!(fp.navigator_platform, Some("Linux armv8l"));
    }

    #[test]
    fn test_crios_empty_client_hints_metadata_absent() {
        // CriOS（WKWebView）无 sec_ch_ua → metadata 整体缺席（绝不发空品牌）；
        // navigator.platform 仍正确映射
        let fp = build_ua_fingerprint(UA_IPHONE_CRIOS, "", "iOS", true);

        assert!(
            fp.metadata.is_none(),
            "WKWebView profile must not send client hints metadata"
        );
        assert_eq!(fp.navigator_platform, Some("iPhone"));
    }

    #[test]
    fn test_ipad_cpu_os_form() {
        let ua = "Mozilla/5.0 (iPad; CPU OS 16_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/128.0.6778.39 Mobile/15E148 Safari/604.1";
        let fp = build_ua_fingerprint(ua, "", "iOS", true);
        assert!(fp.metadata.is_none());
        assert_eq!(fp.navigator_platform, Some("iPad"));
    }

    #[test]
    fn test_linux_desktop_version_unparsable_left_empty() {
        let ua = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
        let fp = build_ua_fingerprint(ua, SEC_CHROME_131, "Linux", false);

        let meta = fp.metadata.expect("metadata present");
        assert_eq!(meta.platform, "Linux");
        // 内核版本不可从 UA 解析——置空而非猜测
        assert_eq!(meta.platform_version, "");
        assert_eq!(fp.navigator_platform, Some("Linux x86_64"));
    }

    #[test]
    fn test_firefox_fallback_platform_labels() {
        // profile 标签缺失时按 UA 兜底（client-hints 词表：macOS 非 Mac OS X）
        let ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
        let fp = build_ua_fingerprint(ua, SEC_CHROME_131, "", false);

        let meta = fp.metadata.expect("metadata present");
        assert_eq!(meta.platform, "macOS");
    }

    #[test]
    fn test_unparsable_ua_deterministic_and_neutral() {
        let fp = build_ua_fingerprint("totally-unrelated-string", "", "", true);
        assert!(fp.metadata.is_none());
        assert_eq!(fp.navigator_platform, None);
        assert_eq!(
            build_ua_fingerprint("totally-unrelated-string", "", "", true),
            fp
        );
    }

    #[test]
    fn test_malformed_sec_ch_ua_defensive() {
        // 残缺模板段（缺 v=/缺引号）跳过不 panic；全残缺 → metadata 缺席
        assert!(
            build_ua_fingerprint(UA_WIN_CHROME, "\"\";v=, broken", "Windows", false)
                .metadata
                .is_none()
        );
        let fp = build_ua_fingerprint(UA_WIN_CHROME, "\"A\";v=\"1\", broken", "Windows", false);
        let brands = fp.metadata.expect("one valid segment").brands.unwrap();
        assert_eq!(brands.len(), 1);
        assert_eq!(brands[0].brand, "A");
    }

    #[test]
    fn test_webview_marker_model_empty() {
        let ua = "Mozilla/5.0 (Linux; Android 6.0; wv) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Mobile Safari/537.36";
        let fp = build_ua_fingerprint(ua, SEC_CHROME_131, "Android", true);
        let meta = fp.metadata.expect("metadata present");
        assert_eq!(meta.model, "", "wv is a WebView marker, not a device model");
    }
}

/// CDP `Emulation.setUserAgentOverride.acceptLanguage` 的输入归一。
///
/// Chromium 重建 Accept-Language 头时会在尾部追加默认权重——传入值自带
/// 尾部 `;q=<权重>` 会产生 `…;q=0.9;q=0.9` 畸形头（活体实测），故剥掉
/// 尾 q 交给浏览器补全；剥后与 wreq 引擎直发的原始模板头形状一致。
/// 无尾 q 的值原样返回。
#[must_use]
pub fn cdp_accept_language(accept_language: &str) -> &str {
    match accept_language.rsplit_once(";q=") {
        Some((head, q)) if !q.is_empty() && q.bytes().all(|b| b.is_ascii_digit() || b == b'.') => {
            head
        }
        _ => accept_language,
    }
}

#[cfg(test)]
mod cdp_accept_language_tests {
    use super::cdp_accept_language;

    #[test]
    fn strips_trailing_q() {
        assert_eq!(cdp_accept_language("en-GB,en;q=0.9"), "en-GB,en");
        assert_eq!(
            cdp_accept_language("zh-CN,zh;q=0.9,en;q=0.5"),
            "zh-CN,zh;q=0.9,en"
        );
    }

    #[test]
    fn keeps_values_without_trailing_q() {
        assert_eq!(cdp_accept_language("en-US,en"), "en-US,en");
        // 尾段非纯权重（含字母）不是 q 参数，不剥
        assert_eq!(
            cdp_accept_language("en-GB,en;q=nonsense"),
            "en-GB,en;q=nonsense"
        );
        assert_eq!(cdp_accept_language(""), "");
    }
}
