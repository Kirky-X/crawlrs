// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! fluxweir 网关集成配置
//!
//! crawlrs 经嵌入式或外部 fluxweir 网关统一 LLM/嵌入/重排出站访问。
//! 嵌入式形态共享 crawlrs 的 dbnexus 连接池（单池共享，见变更
//! `fluxweir-llm-gateway` design D3/D9/D10）；外部形态经 SDK 直连 base_url。
//!
//! 消费方：`bootstrap::fluxweir_gateway`（生命周期）、`init_llm_service`（carrier
//! 装配）、`init_rag_components`（嵌入/重排降级链）。`trusted_proxy` 为保留
//! 开关——当前 SDK 回环形态下 crawlrs 不注入 TrustedIdentity，保持 false；
//! 供未来 HTTP 直连身份桥启用。

use fluent_bundle::FluentValue;
use serde::{Deserialize, Serialize};

/// 配置校验消息出口（双轨基准模式，与 [`crate::config::rag`] 同惯例）
///
/// 启动期 i18n 全局束已初始化时经 Fluent 输出（FTL key 见 `locales/*/fluxweir.ftl`，
/// 语言由检测链/配置决议）；未初始化（单测、轻量面构建）时回退英文规范串。
/// 代码内不再保留中文消息字面量。
fn fluxweir_config_message(
    key: &str,
    english: impl std::fmt::Display,
    args: &[(&str, FluentValue)],
) -> String {
    match crate::i18n::startup_i18n() {
        Some((locale, bundle)) => crate::i18n::t_with_args(locale, bundle, key, args),
        None => english.to_string(),
    }
}

/// fluxweir 网关集成配置
///
/// # 安全提示
///
/// `api_key` 含网关凭据，Debug 输出一律 `[REDACTED]`；运行时密钥解析顺序：
/// 直接值 → [`FluxweirSettings::api_key_env`] 指向的环境变量。
#[derive(Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__FLUXWEIR__")]
pub struct FluxweirSettings {
    /// 总开关：`false` 时不起网关、carrier 维持 ExplicitErrorCarrier，零运行时开销
    #[config(default = false)]
    pub enabled: bool,

    /// 集成形态：`embedded`（进程内起服，共享池）| `external`（SDK 直连外部实例）
    #[config(default = "embedded".to_string())]
    pub mode: String,

    /// 嵌入式网关监听地址（默认仅 loopback；绑 0.0.0.0 需显式配置并自行承担暴露面）
    #[config(default = "127.0.0.1:8900".to_string())]
    pub listen_addr: String,

    /// 网关 API key 的环境变量名（值非本字段）；
    /// 与 [`FluxweirSettings::api_key`] 二选一，均未提供时 validate fail-fast
    #[config(default = String::new())]
    pub api_key_env: String,

    /// fluxweir 完整配置文件路径（整文件优先于内联装配；空 = 纯内联最小装配）
    #[config(default = String::new())]
    pub config_path: String,

    /// 外部网关实例地址（`mode="external"` 必填，如 `http://llm-gw:8080`）；
    /// embedded 模式忽略（监听地址由 listen_addr 承载）
    #[config(default = String::new())]
    pub external_base_url: String,

    /// 可信代理开关（保留机制，缺省 false）：开启时网关接受宿主经 request
    /// extensions 注入的 TrustedIdentity（is_admin 位收窄 /admin/*）。当前
    /// SDK 回环形态下 crawlrs 不注入身份、保持 false 即可——数据面经 SDK
    /// 透传客户端 key 鉴权，与缩围裁决（变更 r3 T020）一致
    #[config(default = false)]
    pub trusted_proxy: bool,

    /// 经网关的 LLM/嵌入/重排请求超时（秒）
    #[config(default = 120)]
    pub request_timeout_secs: u64,

    /// RAG 降级链开关：vecboost（primary）失败时允许切换 fluxweir（fallback）
    #[config(default = true)]
    pub rag_fallback: bool,

    /// 网关 API key 直接值（敏感信息；tests/内联装配用，生产经 api_key_env 注入）
    pub(crate) api_key: Option<String>,
}

impl std::fmt::Debug for FluxweirSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluxweirSettings")
            .field("enabled", &self.enabled)
            .field("mode", &self.mode)
            .field("listen_addr", &self.listen_addr)
            .field("api_key_env", &self.api_key_env)
            .field("config_path", &self.config_path)
            .field("external_base_url", &self.external_base_url)
            .field("trusted_proxy", &self.trusted_proxy)
            .field("request_timeout_secs", &self.request_timeout_secs)
            .field("rag_fallback", &self.rag_fallback)
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

impl FluxweirSettings {
    /// 解析网关 API key：直接值优先，其次 [`FluxweirSettings::api_key_env`]
    /// 指向的环境变量；均不可得时 `None`（调用方按启动期 fail-fast 语义处置）
    pub fn api_key(&self) -> Option<String> {
        if let Some(key) = self.api_key.as_deref() {
            if !key.trim().is_empty() {
                return Some(key.to_string());
            }
        }
        let env_name = self.api_key_env.trim();
        if env_name.is_empty() {
            return None;
        }
        std::env::var(env_name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    }

    /// 启动期配置校验（fail-fast）
    ///
    /// `enabled=false` 全关直通；启用时校验 mode 合法值、嵌入式监听地址、
    /// 外部实例地址、密钥可解析性与超时上界。`mode="external"` 不校验
    /// listen_addr（监听由外部实例自持）。
    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        match self.mode.as_str() {
            "embedded" | "external" => {}
            other => {
                return Err(fluxweir_config_message(
                    "fluxweir-config-mode-invalid",
                    format!(
                        "invalid fluxweir.mode value \"{other}\" (supported: embedded/external)"
                    ),
                    &[("value", FluentValue::from(other))],
                ));
            }
        }

        if self.mode == "embedded" && self.listen_addr.trim().is_empty() {
            return Err(fluxweir_config_message(
                "fluxweir-config-listen-addr-empty",
                "fluxweir.listen_addr cannot be empty when fluxweir.mode = \"embedded\"",
                &[],
            ));
        }

        if self.mode == "external" && self.external_base_url.trim().is_empty() {
            return Err(fluxweir_config_message(
                "fluxweir-config-external-base-url-empty",
                "fluxweir.external_base_url cannot be empty when fluxweir.mode = \"external\"",
                &[],
            ));
        }

        if self.api_key().is_none() {
            return Err(fluxweir_config_message(
                "fluxweir-config-api-key-missing",
                "fluxweir requires an API key: set fluxweir.api_key_env to the environment \
                 variable holding the key, or configure the key directly",
                &[],
            ));
        }

        if self.request_timeout_secs == 0 || self.request_timeout_secs > 600 {
            return Err(fluxweir_config_message(
                "fluxweir-config-timeout-invalid",
                "fluxweir.request_timeout_secs must be within 1..=600",
                &[],
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_all_disabled() {
        let settings = FluxweirSettings::default();
        assert!(!settings.enabled);
        assert_eq!(settings.mode, "embedded");
        assert_eq!(settings.listen_addr, "127.0.0.1:8900");
        assert!(!settings.trusted_proxy);
        assert!(settings.rag_fallback);
        assert_eq!(settings.request_timeout_secs, 120);
        // 默认未启用：validate 直通，不要求密钥
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn test_validate_mode_invalid() {
        let settings = FluxweirSettings {
            enabled: true,
            mode: "sidecar".to_string(),
            api_key: Some("k".to_string()),
            ..FluxweirSettings::default()
        };
        let err = settings.validate().unwrap_err();
        assert!(err.contains("sidecar"), "error must name the value: {err}");
    }

    #[test]
    fn test_validate_enabled_requires_key() {
        let settings = FluxweirSettings {
            enabled: true,
            api_key: None,
            api_key_env: String::new(),
            ..FluxweirSettings::default()
        };
        let err = settings.validate().unwrap_err();
        assert!(
            err.contains("api_key_env") || err.contains("API key"),
            "error must point at key configuration: {err}"
        );
    }

    #[test]
    fn test_validate_embedded_requires_listen_addr() {
        let settings = FluxweirSettings {
            enabled: true,
            mode: "embedded".to_string(),
            listen_addr: String::new(),
            api_key: Some("k".to_string()),
            ..FluxweirSettings::default()
        };
        let err = settings.validate().unwrap_err();
        assert!(
            err.contains("listen_addr"),
            "error must name listen_addr: {err}"
        );
    }

    #[test]
    fn test_validate_external_skips_listen_addr() {
        let settings = FluxweirSettings {
            enabled: true,
            mode: "external".to_string(),
            listen_addr: String::new(),
            external_base_url: "http://llm-gw:8080".to_string(),
            api_key: Some("k".to_string()),
            ..FluxweirSettings::default()
        };
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn test_validate_external_requires_base_url() {
        let settings = FluxweirSettings {
            enabled: true,
            mode: "external".to_string(),
            external_base_url: String::new(),
            api_key: Some("k".to_string()),
            ..FluxweirSettings::default()
        };
        let err = settings.validate().unwrap_err();
        assert!(
            err.contains("external_base_url"),
            "error must name external_base_url: {err}"
        );
    }

    #[test]
    fn test_validate_timeout_bounds() {
        let base = FluxweirSettings {
            enabled: true,
            api_key: Some("k".to_string()),
            ..FluxweirSettings::default()
        };
        let zero = FluxweirSettings {
            request_timeout_secs: 0,
            ..base.clone()
        };
        assert!(zero.validate().is_err());
        let over = FluxweirSettings {
            request_timeout_secs: 601,
            ..base
        };
        assert!(over.validate().is_err());
    }

    #[test]
    fn test_debug_redacts_api_key() {
        // 假夹具口令，验证脱敏行为本身（非真实凭证）
        let settings = FluxweirSettings {
            api_key: Some("super-secret".to_string()), // pragma: allowlist secret
            ..FluxweirSettings::default()
        };
        let rendered = format!("{settings:?}");
        assert!(
            !rendered.contains("super-secret"),
            "debug must redact: {rendered}"
        );
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn test_api_key_resolution_direct_first() {
        let settings = FluxweirSettings {
            api_key: Some("direct".to_string()), // pragma: allowlist secret
            api_key_env: "CRAWLRS_TEST_FLUXWEIR_UNSET_ENV".to_string(), // pragma: allowlist secret
            ..FluxweirSettings::default()
        };
        assert_eq!(settings.api_key().as_deref(), Some("direct"));
    }

    #[test]
    fn test_api_key_resolution_env_fallback() {
        let env_name = "CRAWLRS_TEST_FLUXWEIR_KEY_RESOLVE";
        // 单测进程内独占变量名，避免与并行测试互扰
        std::env::set_var(env_name, "from-env");
        let settings = FluxweirSettings {
            api_key_env: env_name.to_string(),
            ..FluxweirSettings::default()
        };
        assert_eq!(settings.api_key().as_deref(), Some("from-env"));
        std::env::remove_var(env_name);
    }

    #[test]
    fn test_api_key_resolution_empty_direct_falls_through() {
        let settings = FluxweirSettings {
            api_key: Some("   ".to_string()),
            api_key_env: "CRAWLRS_TEST_FLUXWEIR_UNSET_ENV".to_string(), // pragma: allowlist secret
            ..FluxweirSettings::default()
        };
        assert_eq!(settings.api_key(), None);
    }
}
