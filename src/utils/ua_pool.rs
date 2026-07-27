// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! UA Pool — 一致性 User-Agent / Header / Viewport 绑定池
//!
//! 设计目标（specmark crawler-capability-absorption §2 / R-identity-001）：
//! - 内置 ≥20 桌面 + ≥20 移动真实 profile，覆盖 Chrome/Firefox/Safari/Edge
//!   跨 Windows/macOS/Linux/iOS/Android
//! - 每 profile 绑定一致的 UA + Accept-Language + sec-ch-ua + viewport，
//!   避免 JA3 / header 指纹矛盾
//! - `pick_seeded(seed, mobile)` 同 seed 必须稳定返回同一 profile，
//!   用于重试时按 attempt 轮换 UA（与 RetryDirective §4 联动）
//!
//! `DEFAULT_USER_AGENT`（`http_client.rs`）作为 UaPool 不可用时的最终回退常量保留。

use rand::seq::IndexedRandom;

/// 单个 User-Agent profile — 所有字段为 `&'static str`，编译期常量
///
/// 字段绑定关系（避免指纹矛盾）：
/// - `ua` 决定 `sec_ch_ua`（仅 Chromium-based 浏览器发送 Client Hints）
/// - `ua` 决定 `platform`（UA 字符串中的 OS 标识）
/// - `mobile` 决定 `viewport` 范围（mobile 用窄屏，desktop 用宽屏）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UaProfile {
    /// User-Agent 字符串
    pub ua: &'static str,
    /// Accept-Language header 值
    pub accept_language: &'static str,
    /// sec-ch-ua header 值（Chromium-based 浏览器；Firefox/Safari 为空串）
    pub sec_ch_ua: &'static str,
    /// 平台标识（Windows/macOS/Linux/iOS/Android）
    pub platform: &'static str,
    /// 视口尺寸 (width, height)，单位 px
    pub viewport: (u32, u32),
    /// 是否移动端 profile
    pub mobile: bool,
}

/// UA 池 — 桌面 / 移动分组
///
/// 用法：
/// - `pool.pick(request.mobile)` — 随机选 profile
/// - `pool.pick_seeded(attempt as u64, request.mobile)` — 按 attempt 稳定轮换（重试场景）
pub struct UaPool {
    /// 桌面 profile 集（mobile=false）
    pub desktop: Vec<UaProfile>,
    /// 移动 profile 集（mobile=true）
    pub mobile: Vec<UaProfile>,
}

impl UaPool {
    /// 构造内置 ≥20 桌面 + ≥20 移动 profile 的 UA 池
    ///
    /// profile 来源：真实主流浏览器 UA 字符串（Chrome 120-131 / Firefox 128-133 /
    /// Safari 16-17 / Edge 128-131），覆盖 Windows/macOS/Linux/iOS/Android。
    #[must_use]
    pub fn new() -> Self {
        Self {
            desktop: desktop_profiles(),
            mobile: mobile_profiles(),
        }
    }

    /// 随机选取一个 profile
    ///
    /// # Panics
    /// 仅当对应分组为空时 panic（内置 pool 不可能为空，规则 12：失败显性化）
    #[must_use]
    pub fn pick(&self, mobile: bool) -> &UaProfile {
        let pool = if mobile { &self.mobile } else { &self.desktop };
        pool.choose(&mut rand::rng())
            .expect("UA pool group must be non-empty")
    }

    /// 按 seed 确定性选取 profile — 同 seed 必须返回同一 profile
    ///
    /// 用于重试时按 attempt 轮换 UA（避免同一 UA 连续命中反爬）：
    /// ```ignore
    /// let profile = pool.pick_seeded(attempt as u64, request.mobile);
    /// ```
    ///
    /// # Panics
    /// 仅当对应分组为空时 panic
    #[must_use]
    pub fn pick_seeded(&self, seed: u64, mobile: bool) -> &UaProfile {
        let pool = if mobile { &self.mobile } else { &self.desktop };
        let idx = (seed % pool.len() as u64) as usize;
        &pool[idx]
    }

    /// 返回对应分组的 profile 数量
    #[must_use]
    pub fn count(&self, mobile: bool) -> usize {
        if mobile {
            self.mobile.len()
        } else {
            self.desktop.len()
        }
    }
}

impl Default for UaPool {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// 内置 profile 表
// =============================================================================

/// 桌面 profile — 22 个，覆盖 Chrome/Firefox/Safari/Edge × Windows/macOS/Linux
#[must_use]
fn desktop_profiles() -> Vec<UaProfile> {
    // 公共 Accept-Language 集
    const AL_EN_US: &str = "en-US,en;q=0.9";
    const AL_EN_GB: &str = "en-GB,en;q=0.9";
    const AL_ZH_CN: &str = "zh-CN,zh;q=0.9,en;q=0.8";

    // sec-ch-ua 模板（Chromium-based）
    const SEC_CHROME_131: &str =
        "\"Chromium\";v=\"131\", \"Not_A Brand\";v=\"24\", \"Google Chrome\";v=\"131\"";
    const SEC_CHROME_130: &str =
        "\"Chromium\";v=\"130\", \"Not_A Brand\";v=\"24\", \"Google Chrome\";v=\"130\"";
    const SEC_EDGE_131: &str =
        "\"Microsoft Edge\";v=\"131\", \"Chromium\";v=\"131\", \"Not_A Brand\";v=\"24\"";
    const SEC_EDGE_130: &str =
        "\"Microsoft Edge\";v=\"130\", \"Chromium\";v=\"130\", \"Not_A Brand\";v=\"24\"";
    // Firefox / Safari 不发送 sec-ch-ua
    const SEC_NONE: &str = "";

    vec![
        // === Chrome on Windows ===
        UaProfile {
            ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_CHROME_131,
            platform: "Windows",
            viewport: (1920, 1080),
            mobile: false,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
            accept_language: AL_ZH_CN,
            sec_ch_ua: SEC_CHROME_130,
            platform: "Windows",
            viewport: (1366, 768),
            mobile: false,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36",
            accept_language: AL_EN_US,
            sec_ch_ua: "\"Chromium\";v=\"128\", \"Not_A Brand\";v=\"24\", \"Google Chrome\";v=\"128\"",
            platform: "Windows",
            viewport: (1536, 864),
            mobile: false,
        },
        // === Chrome on macOS ===
        UaProfile {
            ua: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_CHROME_131,
            platform: "macOS",
            viewport: (1920, 1080),
            mobile: false,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
            accept_language: AL_ZH_CN,
            sec_ch_ua: SEC_CHROME_130,
            platform: "macOS",
            viewport: (1440, 900),
            mobile: false,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
            accept_language: AL_EN_GB,
            sec_ch_ua: "\"Chromium\";v=\"125\", \"Not_A Brand\";v=\"24\", \"Google Chrome\";v=\"125\"",
            platform: "macOS",
            viewport: (1680, 1050),
            mobile: false,
        },
        // === Chrome on Linux ===
        UaProfile {
            ua: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_CHROME_131,
            platform: "Linux",
            viewport: (1920, 1080),
            mobile: false,
        },
        UaProfile {
            ua: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
            accept_language: AL_EN_GB,
            sec_ch_ua: SEC_CHROME_130,
            platform: "Linux",
            viewport: (1366, 768),
            mobile: false,
        },
        // === Firefox on Windows ===
        UaProfile {
            ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:133.0) Gecko/20100101 Firefox/133.0",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_NONE,
            platform: "Windows",
            viewport: (1920, 1080),
            mobile: false,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:130.0) Gecko/20100101 Firefox/130.0",
            accept_language: AL_ZH_CN,
            sec_ch_ua: SEC_NONE,
            platform: "Windows",
            viewport: (1366, 768),
            mobile: false,
        },
        // === Firefox on macOS ===
        UaProfile {
            ua: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:133.0) Gecko/20100101 Firefox/133.0",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_NONE,
            platform: "macOS",
            viewport: (1440, 900),
            mobile: false,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:128.0) Gecko/20100101 Firefox/128.0",
            accept_language: AL_EN_GB,
            sec_ch_ua: SEC_NONE,
            platform: "macOS",
            viewport: (1280, 800),
            mobile: false,
        },
        // === Firefox on Linux ===
        UaProfile {
            ua: "Mozilla/5.0 (X11; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_NONE,
            platform: "Linux",
            viewport: (1920, 1080),
            mobile: false,
        },
        UaProfile {
            ua: "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0",
            accept_language: AL_ZH_CN,
            sec_ch_ua: SEC_NONE,
            platform: "Linux",
            viewport: (1280, 800),
            mobile: false,
        },
        // === Safari on macOS (无 sec-ch-ua) ===
        UaProfile {
            ua: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Safari/605.1.15",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_NONE,
            platform: "macOS",
            viewport: (1920, 1080),
            mobile: false,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.1 Safari/605.1.15",
            accept_language: AL_EN_GB,
            sec_ch_ua: SEC_NONE,
            platform: "macOS",
            viewport: (1440, 900),
            mobile: false,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.6 Safari/605.1.15",
            accept_language: AL_ZH_CN,
            sec_ch_ua: SEC_NONE,
            platform: "macOS",
            viewport: (1280, 800),
            mobile: false,
        },
        // === Edge on Windows ===
        UaProfile {
            ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_EDGE_131,
            platform: "Windows",
            viewport: (1920, 1080),
            mobile: false,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0",
            accept_language: AL_ZH_CN,
            sec_ch_ua: SEC_EDGE_130,
            platform: "Windows",
            viewport: (1366, 768),
            mobile: false,
        },
        // === Edge on macOS ===
        UaProfile {
            ua: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_EDGE_131,
            platform: "macOS",
            viewport: (1440, 900),
            mobile: false,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36 Edg/128.0.0.0",
            accept_language: AL_EN_GB,
            sec_ch_ua: "\"Microsoft Edge\";v=\"128\", \"Chromium\";v=\"128\", \"Not_A Brand\";v=\"24\"",
            platform: "macOS",
            viewport: (1280, 800),
            mobile: false,
        },
        // === Chrome on Windows (补充至 22) ===
        UaProfile {
            ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/127.0.0.0 Safari/537.36",
            accept_language: AL_EN_US,
            sec_ch_ua: "\"Chromium\";v=\"127\", \"Not_A Brand\";v=\"24\", \"Google Chrome\";v=\"127\"",
            platform: "Windows",
            viewport: (1920, 1080),
            mobile: false,
        },
    ]
}

/// 移动 profile — 22 个，覆盖 Chrome/Safari/Firefox/Edge × iOS/Android
#[must_use]
fn mobile_profiles() -> Vec<UaProfile> {
    const AL_EN_US: &str = "en-US,en;q=0.9";
    const AL_EN_GB: &str = "en-GB,en;q=0.9";
    const AL_ZH_CN: &str = "zh-CN,zh;q=0.9,en;q=0.8";

    const SEC_CHROME_131: &str =
        "\"Chromium\";v=\"131\", \"Not_A Brand\";v=\"24\", \"Google Chrome\";v=\"131\"";
    const SEC_CHROME_130: &str =
        "\"Chromium\";v=\"130\", \"Not_A Brand\";v=\"24\", \"Google Chrome\";v=\"130\"";
    const SEC_EDGE_131: &str =
        "\"Microsoft Edge\";v=\"131\", \"Chromium\";v=\"131\", \"Not_A Brand\";v=\"24\"";
    const SEC_SAMSUNG: &str =
        "\"Samsung Internet\";v=\"23\", \"Chromium\";v=\"120\", \"Not_A Brand\";v=\"24\"";
    const SEC_NONE: &str = "";

    vec![
        // === Chrome on Android ===
        UaProfile {
            ua: "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_CHROME_131,
            platform: "Android",
            viewport: (412, 915),
            mobile: true,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Linux; Android 13; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36",
            accept_language: AL_ZH_CN,
            sec_ch_ua: SEC_CHROME_130,
            platform: "Android",
            viewport: (360, 780),
            mobile: true,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Linux; Android 12; SM-G991B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Mobile Safari/537.36",
            accept_language: AL_EN_US,
            sec_ch_ua: "\"Chromium\";v=\"128\", \"Not_A Brand\";v=\"24\", \"Google Chrome\";v=\"128\"",
            platform: "Android",
            viewport: (384, 640),
            mobile: true,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Linux; Android 14; SM-S928U) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36",
            accept_language: AL_EN_GB,
            sec_ch_ua: SEC_CHROME_131,
            platform: "Android",
            viewport: (360, 800),
            mobile: true,
        },
        // === Chrome on iOS (iPhone) ===
        UaProfile {
            ua: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/131.0.6778.73 Mobile/15E148 Safari/604.1",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_CHROME_131,
            platform: "iOS",
            viewport: (393, 852),
            mobile: true,
        },
        UaProfile {
            ua: "Mozilla/5.0 (iPhone; CPU iPhone OS 16_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/130.0.6778.39 Mobile/15E148 Safari/604.1",
            accept_language: AL_ZH_CN,
            sec_ch_ua: SEC_CHROME_130,
            platform: "iOS",
            viewport: (390, 844),
            mobile: true,
        },
        // === Safari on iOS (iPhone) ===
        UaProfile {
            ua: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Mobile/15E148 Safari/604.1",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_NONE,
            platform: "iOS",
            viewport: (393, 852),
            mobile: true,
        },
        UaProfile {
            ua: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.1 Mobile/15E148 Safari/604.1",
            accept_language: AL_EN_GB,
            sec_ch_ua: SEC_NONE,
            platform: "iOS",
            viewport: (390, 844),
            mobile: true,
        },
        UaProfile {
            ua: "Mozilla/5.0 (iPhone; CPU iPhone OS 16_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.6 Mobile/15E148 Safari/604.1",
            accept_language: AL_ZH_CN,
            sec_ch_ua: SEC_NONE,
            platform: "iOS",
            viewport: (375, 812),
            mobile: true,
        },
        UaProfile {
            ua: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Mobile/15E148 Safari/604.1",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_NONE,
            platform: "iOS",
            viewport: (390, 844),
            mobile: true,
        },
        // === Safari on iPad ===
        UaProfile {
            ua: "Mozilla/5.0 (iPad; CPU OS 17_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Mobile/15E148 Safari/604.1",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_NONE,
            platform: "iOS",
            viewport: (820, 1180),
            mobile: true,
        },
        UaProfile {
            ua: "Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1",
            accept_language: AL_ZH_CN,
            sec_ch_ua: SEC_NONE,
            platform: "iOS",
            viewport: (1024, 1366),
            mobile: true,
        },
        // === Firefox on Android ===
        UaProfile {
            ua: "Mozilla/5.0 (Android 14; Mobile; rv:133.0) Gecko/133.0 Firefox/133.0",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_NONE,
            platform: "Android",
            viewport: (360, 800),
            mobile: true,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Android 13; Mobile; rv:131.0) Gecko/131.0 Firefox/131.0",
            accept_language: AL_ZH_CN,
            sec_ch_ua: SEC_NONE,
            platform: "Android",
            viewport: (412, 915),
            mobile: true,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Android 12; Mobile; rv:128.0) Gecko/128.0 Firefox/128.0",
            accept_language: AL_EN_GB,
            sec_ch_ua: SEC_NONE,
            platform: "Android",
            viewport: (360, 780),
            mobile: true,
        },
        // === Firefox on iOS ===
        UaProfile {
            ua: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) FxiOS/133.0 Mobile/15E148 Safari/605.1.15",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_NONE,
            platform: "iOS",
            viewport: (390, 844),
            mobile: true,
        },
        UaProfile {
            ua: "Mozilla/5.0 (iPhone; CPU iPhone OS 16_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) FxiOS/130.0 Mobile/15E148 Safari/605.1.15",
            accept_language: AL_ZH_CN,
            sec_ch_ua: SEC_NONE,
            platform: "iOS",
            viewport: (393, 852),
            mobile: true,
        },
        // === Edge on Android ===
        UaProfile {
            ua: "Mozilla/5.0 (Linux; Android 14; SM-S928U) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36 EdgA/131.0.0.0",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_EDGE_131,
            platform: "Android",
            viewport: (360, 800),
            mobile: true,
        },
        UaProfile {
            ua: "Mozilla/5.0 (Linux; Android 13; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Mobile Safari/537.36 EdgA/128.0.0.0",
            accept_language: AL_ZH_CN,
            sec_ch_ua: "\"Microsoft Edge\";v=\"128\", \"Chromium\";v=\"128\", \"Not_A Brand\";v=\"24\"",
            platform: "Android",
            viewport: (412, 915),
            mobile: true,
        },
        // === Edge on iOS ===
        UaProfile {
            ua: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) EdgiOS/131.0.2592.73 Mobile/15E148 Safari/605.1.15",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_EDGE_131,
            platform: "iOS",
            viewport: (390, 844),
            mobile: true,
        },
        // === Chrome on Android Tablet ===
        UaProfile {
            ua: "Mozilla/5.0 (Linux; Android 13; SM-X910N) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
            accept_language: AL_EN_GB,
            sec_ch_ua: SEC_CHROME_130,
            platform: "Android",
            viewport: (800, 1280),
            mobile: true,
        },
        // === Samsung Internet on Android ===
        UaProfile {
            ua: "Mozilla/5.0 (Linux; Android 14; SM-S928B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/23.0 Chrome/115.0.0.0 Mobile Safari/537.36",
            accept_language: AL_EN_US,
            sec_ch_ua: SEC_SAMSUNG,
            platform: "Android",
            viewport: (360, 780),
            mobile: true,
        },
    ]
}

// =============================================================================
// Tests — TDD: 验证 R-identity-001 验收标准
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // === R-identity-001: profile 数量 ≥ 20 ===

    #[test]
    fn test_desktop_pool_has_at_least_20_profiles() {
        let pool = UaPool::new();
        assert!(
            pool.desktop.len() >= 20,
            "desktop pool must have >= 20 profiles, got {}",
            pool.desktop.len()
        );
    }

    #[test]
    fn test_mobile_pool_has_at_least_20_profiles() {
        let pool = UaPool::new();
        assert!(
            pool.mobile.len() >= 20,
            "mobile pool must have >= 20 profiles, got {}",
            pool.mobile.len()
        );
    }

    // === R-identity-001: 桌面 profile 不配移动 viewport / mobile 标志 ===

    #[test]
    fn test_desktop_profiles_have_mobile_false() {
        let pool = UaPool::new();
        for (i, p) in pool.desktop.iter().enumerate() {
            assert!(
                !p.mobile,
                "desktop profile #{} has mobile=true (UA: {})",
                i,
                p.ua
            );
        }
    }

    #[test]
    fn test_desktop_profiles_have_desktop_viewport() {
        let pool = UaPool::new();
        for (i, p) in pool.desktop.iter().enumerate() {
            // 桌面 viewport 宽度应 >= 1024（排除平板/移动）
            assert!(
                p.viewport.0 >= 1024,
                "desktop profile #{} has mobile-like viewport {:?} (UA: {})",
                i,
                p.viewport,
                p.ua
            );
        }
    }

    #[test]
    fn test_desktop_uas_do_not_contain_mobile_marker() {
        let pool = UaPool::new();
        for (i, p) in pool.desktop.iter().enumerate() {
            // 桌面 UA 不应包含 "Mobile" 或 "Android" 或 "iPhone"
            let ua_lower = p.ua.to_lowercase();
            assert!(
                !ua_lower.contains("mobile") && !ua_lower.contains("iphone"),
                "desktop profile #{} UA contains mobile marker: {}",
                i,
                p.ua
            );
        }
    }

    // === R-identity-001: 移动 profile mobile=true 且 viewport 合理 ===

    #[test]
    fn test_mobile_profiles_have_mobile_true() {
        let pool = UaPool::new();
        for (i, p) in pool.mobile.iter().enumerate() {
            assert!(
                p.mobile,
                "mobile profile #{} has mobile=false (UA: {})",
                i,
                p.ua
            );
        }
    }

    #[test]
    fn test_mobile_profiles_have_mobile_viewport() {
        let pool = UaPool::new();
        for (i, p) in pool.mobile.iter().enumerate() {
            // 移动 viewport 宽度应 < 1024（iPhone/iPad/Android 手机）
            // iPad 平板允许宽度到 1024，但应 < 1366
            assert!(
                p.viewport.0 < 1366,
                "mobile profile #{} has desktop-like viewport {:?} (UA: {})",
                i,
                p.viewport,
                p.ua
            );
        }
    }

    #[test]
    fn test_mobile_uas_contain_mobile_marker() {
        let pool = UaPool::new();
        for (i, p) in pool.mobile.iter().enumerate() {
            let ua_lower = p.ua.to_lowercase();
            let has_marker = ua_lower.contains("mobile")
                || ua_lower.contains("iphone")
                || ua_lower.contains("android");
            assert!(
                has_marker,
                "mobile profile #{} UA lacks mobile marker: {}",
                i,
                p.ua
            );
        }
    }

    // === R-identity-001: 浏览器覆盖度（Chrome/Firefox/Safari/Edge）===

    #[test]
    fn test_desktop_pool_covers_all_major_browsers() {
        let pool = UaPool::new();
        let has_chrome = pool.desktop.iter().any(|p| {
            p.ua.contains("Chrome") && !p.ua.contains("Edg") && !p.ua.contains("CriOS")
        });
        let has_firefox = pool.desktop.iter().any(|p| p.ua.contains("Firefox"));
        let has_safari = pool
            .desktop
            .iter()
            .any(|p| p.ua.contains("Safari") && !p.ua.contains("Chrome"));
        let has_edge = pool.desktop.iter().any(|p| p.ua.contains("Edg"));
        assert!(has_chrome, "desktop pool must cover Chrome");
        assert!(has_firefox, "desktop pool must cover Firefox");
        assert!(has_safari, "desktop pool must cover Safari");
        assert!(has_edge, "desktop pool must cover Edge");
    }

    #[test]
    fn test_mobile_pool_covers_all_major_browsers() {
        let pool = UaPool::new();
        let has_chrome = pool.mobile.iter().any(|p| {
            (p.ua.contains("Chrome") || p.ua.contains("CriOS")) && !p.ua.contains("Edg")
        });
        let has_firefox = pool.mobile.iter().any(|p| p.ua.contains("FxiOS") || p.ua.contains("Firefox"));
        let has_safari = pool
            .mobile
            .iter()
            .any(|p| p.ua.contains("Safari") && !p.ua.contains("Chrome"));
        let has_edge = pool.mobile.iter().any(|p| p.ua.contains("Edg"));
        assert!(has_chrome, "mobile pool must cover Chrome");
        assert!(has_firefox, "mobile pool must cover Firefox");
        assert!(has_safari, "mobile pool must cover Safari");
        assert!(has_edge, "mobile pool must cover Edge");
    }

    // === R-identity-001: 平台覆盖度（Windows/macOS/Linux/iOS/Android）===

    #[test]
    fn test_desktop_pool_covers_major_platforms() {
        let pool = UaPool::new();
        let platforms: std::collections::HashSet<&str> =
            pool.desktop.iter().map(|p| p.platform).collect();
        assert!(platforms.contains("Windows"), "must cover Windows");
        assert!(platforms.contains("macOS"), "must cover macOS");
        assert!(platforms.contains("Linux"), "must cover Linux");
    }

    #[test]
    fn test_mobile_pool_covers_major_platforms() {
        let pool = UaPool::new();
        let platforms: std::collections::HashSet<&str> =
            pool.mobile.iter().map(|p| p.platform).collect();
        assert!(platforms.contains("iOS"), "must cover iOS");
        assert!(platforms.contains("Android"), "must cover Android");
    }

    // === R-identity-001: sec-ch-ua 一致性（Chromium→非空，Firefox/Safari→空）===

    #[test]
    fn test_sec_ch_ua_consistency_chromium() {
        let pool = UaPool::new();
        for p in pool.desktop.iter().chain(pool.mobile.iter()) {
            let is_chromium = p.ua.contains("Chrome")
                || p.ua.contains("CriOS")
                || p.ua.contains("Edg")
                || p.ua.contains("SamsungBrowser");
            if is_chromium {
                assert!(
                    !p.sec_ch_ua.is_empty(),
                    "Chromium UA must have non-empty sec_ch_ua: {}",
                    p.ua
                );
            }
        }
    }

    #[test]
    fn test_sec_ch_ua_consistency_non_chromium() {
        let pool = UaPool::new();
        for p in pool.desktop.iter().chain(pool.mobile.iter()) {
            let is_chromium = p.ua.contains("Chrome")
                || p.ua.contains("CriOS")
                || p.ua.contains("Edg")
                || p.ua.contains("SamsungBrowser");
            let is_firefox = p.ua.contains("Firefox") || p.ua.contains("FxiOS");
            // Safari 唯一标识：含 "Version/" 且非 Chromium/Firefox 变体
            // （iOS 上 Chrome=criOS、Firefox=FxiOS、Edge=EdgiOS 都带 "Safari" 但不带 "Version/"）
            let is_safari = p.ua.contains("Version/") && !is_chromium && !is_firefox;
            if is_firefox || is_safari {
                assert!(
                    p.sec_ch_ua.is_empty(),
                    "Firefox/Safari UA must have empty sec_ch_ua: {}",
                    p.ua
                );
            }
        }
    }

    // === R-identity-001: viewport 与 platform 一致 ===

    #[test]
    fn test_ios_profiles_have_ios_viewport() {
        let pool = UaPool::new();
        for p in pool.mobile.iter() {
            if p.platform == "iOS" {
                // iPhone/iPad viewport 宽度典型值：375/390/393/414/768/810/820/1024
                let w = p.viewport.0;
                assert!(
                    (375..=1366).contains(&w),
                    "iOS profile viewport width {} out of expected range: {}",
                    w,
                    p.ua
                );
            }
        }
    }

    #[test]
    fn test_android_profiles_have_android_viewport() {
        let pool = UaPool::new();
        for p in pool.mobile.iter() {
            if p.platform == "Android" {
                let w = p.viewport.0;
                // Android 手机/平板典型宽度：360/384/412/800
                assert!(
                    (360..=1280).contains(&w),
                    "Android profile viewport width {} out of expected range: {}",
                    w,
                    p.ua
                );
            }
        }
    }

    // === R-identity-001: pick() 返回有效 profile ===

    #[test]
    fn test_pick_desktop_returns_desktop_profile() {
        let pool = UaPool::new();
        let p = pool.pick(false);
        assert!(!p.mobile, "pick(false) must return desktop profile");
    }

    #[test]
    fn test_pick_mobile_returns_mobile_profile() {
        let pool = UaPool::new();
        let p = pool.pick(true);
        assert!(p.mobile, "pick(true) must return mobile profile");
    }

    #[test]
    fn test_pick_returns_varied_profiles() {
        // 多次 pick 应有概率返回不同 profile（验证随机性，非纯 round-robin）
        let pool = UaPool::new();
        let mut uas = std::collections::HashSet::new();
        for _ in 0..50 {
            uas.insert(pool.pick(false).ua);
        }
        // 50 次随机选取应至少命中 2 个不同 profile（统计上几乎必然）
        assert!(
            uas.len() >= 2,
            "pick() should return varied profiles, got only {} unique in 50 picks",
            uas.len()
        );
    }

    // === R-identity-001: pick_seeded() 同 seed 稳定返回 ===

    #[test]
    fn test_pick_seeded_is_deterministic_same_seed() {
        let pool = UaPool::new();
        let seed = 42_u64;
        let p1 = pool.pick_seeded(seed, false);
        let p2 = pool.pick_seeded(seed, false);
        let p3 = pool.pick_seeded(seed, false);
        assert_eq!(
            p1.ua, p2.ua,
            "same seed must return same profile (desktop)"
        );
        assert_eq!(
            p2.ua, p3.ua,
            "same seed must return same profile (desktop, third call)"
        );
    }

    #[test]
    fn test_pick_seeded_is_deterministic_mobile() {
        let pool = UaPool::new();
        let seed = 99_u64;
        let p1 = pool.pick_seeded(seed, true);
        let p2 = pool.pick_seeded(seed, true);
        assert_eq!(p1.ua, p2.ua, "same seed must return same profile (mobile)");
    }

    #[test]
    fn test_pick_seeded_different_seeds_return_different_profiles() {
        // 当 pool 大小 > 1 时，不同 seed 应返回不同 profile（至少存在一对）
        let pool = UaPool::new();
        let mut uas = std::collections::HashSet::new();
        for seed in 0..pool.desktop.len() as u64 {
            uas.insert(pool.pick_seeded(seed, false).ua);
        }
        assert_eq!(
            uas.len(),
            pool.desktop.len(),
            "different seeds (0..len) must return all distinct profiles"
        );
    }

    #[test]
    fn test_pick_seeded_returns_correct_group() {
        let pool = UaPool::new();
        for seed in [0_u64, 1, 5, 100, u64::MAX] {
            assert!(
                !pool.pick_seeded(seed, false).mobile,
                "pick_seeded(_, false) must return desktop profile (seed={})",
                seed
            );
            assert!(
                pool.pick_seeded(seed, true).mobile,
                "pick_seeded(_, true) must return mobile profile (seed={})",
                seed
            );
        }
    }

    #[test]
    fn test_pick_seeded_wraps_around_pool_size() {
        // seed 超过 pool.len() 时应 wrap（取模），不 panic
        let pool = UaPool::new();
        let _ = pool.pick_seeded(u64::MAX, false);
        let _ = pool.pick_seeded(u64::MAX, true);
        let _ = pool.pick_seeded(0, false);
        let _ = pool.pick_seeded(0, true);
    }

    // === 辅助方法测试 ===

    #[test]
    fn test_count_returns_correct_size() {
        let pool = UaPool::new();
        assert_eq!(pool.count(false), pool.desktop.len());
        assert_eq!(pool.count(true), pool.mobile.len());
    }

    #[test]
    fn test_default_eq_new() {
        let a = UaPool::default();
        let b = UaPool::new();
        assert_eq!(a.desktop.len(), b.desktop.len());
        assert_eq!(a.mobile.len(), b.mobile.len());
        // 内容应完全一致（静态表）
        assert_eq!(a.desktop[0].ua, b.desktop[0].ua);
        assert_eq!(a.mobile[0].ua, b.mobile[0].ua);
    }

    // === 一致性验证：UA / sec_ch_ua / platform 不矛盾 ===

    #[test]
    fn test_all_profiles_have_consistent_platform_in_ua() {
        let pool = UaPool::new();
        for p in pool.desktop.iter().chain(pool.mobile.iter()) {
            let ua_lower = p.ua.to_lowercase();
            let platform_consistent = match p.platform {
                "Windows" => ua_lower.contains("windows"),
                "macOS" => ua_lower.contains("macintosh") || ua_lower.contains("mac os x"),
                "Linux" => ua_lower.contains("linux") || ua_lower.contains("x11"),
                "iOS" => ua_lower.contains("iphone") || ua_lower.contains("ipad"),
                "Android" => ua_lower.contains("android"),
                _ => false,
            };
            assert!(
                platform_consistent,
                "platform '{}' not consistent with UA: {}",
                p.platform, p.ua
            );
        }
    }

    #[test]
    fn test_all_profiles_have_non_empty_fields() {
        let pool = UaPool::new();
        for p in pool.desktop.iter().chain(pool.mobile.iter()) {
            assert!(!p.ua.is_empty(), "UA must be non-empty");
            assert!(
                !p.accept_language.is_empty(),
                "Accept-Language must be non-empty"
            );
            assert!(!p.platform.is_empty(), "platform must be non-empty");
            // sec_ch_ua 可以为空（Firefox/Safari）
            assert!(
                p.viewport.0 > 0 && p.viewport.1 > 0,
                "viewport must be non-zero: {:?}",
                p.viewport
            );
        }
    }
}
