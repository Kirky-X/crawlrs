// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Security posture doctor (`crawlrs doctor --security`).
//!
//! Compile-time checks probe the feature matrix (auth/webhook/rate-limit/llm)
//! and the SSRF kill-switch environment variable; config-based checks inspect
//! runtime settings (rate limiting, proxy egress, LLM sanitization). Output is
//! a three-state (✓/✗/!) checklist; any `Fail` yields exit code 1.

use crate::config::settings::Settings;

/// Env var that disables SSRF dual validation (dangerous, doctor fails it).
const SSRF_DISABLE_ENV: &str = "CRAWLRS_DISABLE_SSRF_PROTECTION";

/// Three-state check result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    /// Capability present / setting sane.
    Ok,
    /// Missing but optional hardening (degraded posture).
    Warn,
    /// Dangerous state (SSRF protection off in a non-test build).
    Fail,
}

impl CheckStatus {
    /// Line prefix for text rendering.
    pub fn prefix(&self) -> char {
        match self {
            CheckStatus::Ok => '✓',
            CheckStatus::Warn => '!',
            CheckStatus::Fail => '✗',
        }
    }
}

/// One doctor check line.
#[derive(Debug, Clone)]
pub struct DoctorCheck {
    /// Stable check name (e.g. `auth_garrison`).
    pub name: &'static str,
    /// Outcome.
    pub status: CheckStatus,
    /// Human-readable explanation.
    pub detail: String,
}

/// Aggregated posture: all checks plus exit-code and rendering helpers.
#[derive(Debug, Clone)]
pub struct SecurityPosture {
    /// All check lines (compile-time + config-based).
    pub checks: Vec<DoctorCheck>,
}

impl SecurityPosture {
    /// Full posture: compile-time + config-based checks combined.
    pub fn full(settings: &Settings) -> Self {
        Self {
            checks: [compile_time(), config_based(settings)].concat(),
        }
    }

    /// Exit code: any `Fail` → 1; warnings alone → 0.
    pub fn exit_code(&self) -> i32 {
        if self.checks.iter().any(|c| c.status == CheckStatus::Fail) {
            1
        } else {
            0
        }
    }

    /// Render as prefixed text lines (one per check).
    pub fn render(&self) -> String {
        self.checks
            .iter()
            .map(|c| format!("{} {}: {}", c.status.prefix(), c.name, c.detail))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Compile-time capability checks (feature matrix + SSRF kill switch).
///
/// The SSRF env check is skipped under `cfg(test)` so tests of the other
/// checks stay deterministic regardless of the ambient environment.
pub fn compile_time() -> Vec<DoctorCheck> {
    let mut checks = vec![
        DoctorCheck {
            name: "auth_garrison",
            status: feature_status(cfg!(feature = "auth")),
            detail: feature_detail(
                cfg!(feature = "auth"),
                "Garrison authentication compiled in",
                "auth feature not compiled: protected routes run auth-off",
            ),
        },
        DoctorCheck {
            name: "webhook",
            status: feature_status(cfg!(feature = "webhook")),
            detail: feature_detail(
                cfg!(feature = "webhook"),
                "Webhook delivery compiled in",
                "webhook feature not compiled",
            ),
        },
        DoctorCheck {
            name: "rate_limit_limiteron",
            status: feature_status(cfg!(feature = "rate-limit")),
            detail: feature_detail(
                cfg!(feature = "rate-limit"),
                "limiteron rate limiting compiled in",
                "rate-limit feature not compiled: no request throttling",
            ),
        },
        DoctorCheck {
            name: "llm_boundary",
            status: feature_status(cfg!(feature = "llm")),
            detail: feature_detail(
                cfg!(feature = "llm"),
                "LLM extraction boundary compiled in",
                "llm feature not compiled: LLM extraction unavailable",
            ),
        },
    ];

    let ssrf_disabled = std::env::var(SSRF_DISABLE_ENV)
        .map(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
        .unwrap_or(false);
    if !cfg!(test) {
        checks.push(DoctorCheck {
            name: "ssrf_dual_validation",
            status: if ssrf_disabled {
                CheckStatus::Fail
            } else {
                CheckStatus::Ok
            },
            detail: if ssrf_disabled {
                format!("{SSRF_DISABLE_ENV} is set: SSRF dual validation disabled")
            } else {
                "SSRF dual validation active (handler + engine routing)".to_string()
            },
        });
    }
    checks
}

/// Runtime configuration checks.
pub fn config_based(settings: &Settings) -> Vec<DoctorCheck> {
    let mut checks = vec![
        DoctorCheck {
            name: "rate_limit_config",
            status: if settings.rate_limiting.enabled {
                CheckStatus::Ok
            } else {
                CheckStatus::Warn
            },
            detail: if settings.rate_limiting.enabled {
                format!(
                    "rate limiting on (default {} rpm)",
                    settings.rate_limiting.default_rpm
                )
            } else {
                "rate_limiting.enabled = false: unthrottled API surface".to_string()
            },
        },
        DoctorCheck {
            name: "proxy_egress",
            status: if settings.proxy.urls.is_empty() {
                CheckStatus::Warn
            } else {
                CheckStatus::Ok
            },
            detail: if settings.proxy.urls.is_empty() {
                "no [proxy] URLs configured: engines egress directly".to_string()
            } else {
                format!("{} proxy URL(s) configured", settings.proxy.urls.len())
            },
        },
    ];

    // Per-tenant outbound isolation posture.
    let tenant_count = settings.outbound_isolation.len();
    checks.push(DoctorCheck {
        name: "outbound_isolation",
        status: if tenant_count == 0 {
            CheckStatus::Warn
        } else {
            CheckStatus::Ok
        },
        detail: if tenant_count == 0 {
            "no [outbound_isolation] tenants: all tenants share the proxy pool".to_string()
        } else {
            let mut tenants: Vec<&str> = settings
                .outbound_isolation
                .keys()
                .map(String::as_str)
                .collect();
            tenants.sort_unstable();
            format!(
                "{tenant_count} tenant(s) configured: {}",
                tenants.join(", ")
            )
        },
    });

    // Compliance profile posture.
    let compliance_count = settings.compliance.len();
    checks.push(DoctorCheck {
        name: "compliance_profile",
        status: if compliance_count == 0 {
            CheckStatus::Warn
        } else {
            CheckStatus::Ok
        },
        detail: if compliance_count == 0 {
            "no [compliance] profiles configured".to_string()
        } else {
            format!("{compliance_count} compliance profile(s) configured")
        },
    });

    // LLM sanitization probe (delivered by the llm-injection-sanitizer change):
    // warn when the LLM boundary is compiled but input sanitization is off.
    if cfg!(feature = "llm") {
        let san = &settings.llm.sanitization;
        checks.push(DoctorCheck {
            name: "llm_sanitization",
            status: if san.enabled {
                CheckStatus::Ok
            } else {
                CheckStatus::Warn
            },
            detail: if san.enabled {
                "LLM injection sanitization on ([llm.sanitization])".to_string()
            } else {
                "llm.sanitization.enabled = false: prompt-injection carriers reach the LLM"
                    .to_string()
            },
        });
    }
    checks
}

fn feature_status(on: bool) -> CheckStatus {
    if on {
        CheckStatus::Ok
    } else {
        CheckStatus::Warn
    }
}

fn feature_detail(on: bool, ok: &str, off: &str) -> String {
    if on {
        ok.to_string()
    } else {
        off.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Full-feature build: auth/webhook/rate-limit compile-time checks are Ok.
    #[test]
    fn compile_time_full_feature_auth_webhook_ratelimit_ok() {
        let checks = compile_time();
        let by_name = |n: &str| checks.iter().find(|c| c.name == n).unwrap();
        assert_eq!(by_name("auth_garrison").status, CheckStatus::Ok);
        assert_eq!(by_name("webhook").status, CheckStatus::Ok);
        assert_eq!(by_name("rate_limit_limiteron").status, CheckStatus::Ok);
    }

    /// Default settings render contains the two Warn lines and exit code 0.
    #[test]
    fn config_based_default_settings_two_warns_exit_zero() {
        let settings = Settings::default();
        let posture = SecurityPosture::full(&settings);
        let rendered = posture.render();
        // Default proxy is empty → Warn; rate limiting default on → Ok.
        assert!(
            rendered.contains("! proxy_egress"),
            "expected proxy warn line, got: {rendered}"
        );
        assert!(
            !rendered.contains("✗"),
            "no Fail lines expected: {rendered}"
        );
        assert_eq!(posture.exit_code(), 0);
        // Every line carries one of the three prefixes.
        assert!(rendered.lines().count() >= 5);
        for line in rendered.lines() {
            assert!(
                line.starts_with("✓ ") || line.starts_with("! ") || line.starts_with("✗ "),
                "missing status prefix: {line}"
            );
        }
    }

    /// A Fail check (simulated SSRF env) drives exit code to 1.
    #[test]
    fn any_fail_drives_exit_code_one() {
        let posture = SecurityPosture {
            checks: vec![
                DoctorCheck {
                    name: "ok_check",
                    status: CheckStatus::Ok,
                    detail: "fine".to_string(),
                },
                DoctorCheck {
                    name: "fail_check",
                    status: CheckStatus::Fail,
                    detail: "broken".to_string(),
                },
            ],
        };
        assert_eq!(posture.exit_code(), 1);
    }

    /// Sanitization disabled (llm build) surfaces the warn probe.
    #[test]
    fn config_based_llm_sanitization_off_warns() {
        let mut settings = Settings::default();
        settings.llm.sanitization.enabled = false;
        let checks = config_based(&settings);
        let san = checks.iter().find(|c| c.name == "llm_sanitization");
        if cfg!(feature = "llm") {
            let san = san.unwrap();
            assert_eq!(san.status, CheckStatus::Warn);
        } else {
            assert!(san.is_none());
        }
    }

    /// Empty outbound isolation warns; a configured tenant is reported as Ok.
    #[test]
    fn config_based_outbound_isolation_check() {
        let settings = Settings::default();
        let checks = config_based(&settings);
        let iso = checks
            .iter()
            .find(|c| c.name == "outbound_isolation")
            .unwrap();
        assert_eq!(iso.status, CheckStatus::Warn);
        assert!(iso.detail.contains("share the proxy pool"));

        let mut settings = Settings::default();
        settings.outbound_isolation.insert(
            "acme".to_string(),
            crate::config::outbound::TenantOutboundConfig::default(),
        );
        let checks = config_based(&settings);
        let iso = checks
            .iter()
            .find(|c| c.name == "outbound_isolation")
            .unwrap();
        assert_eq!(iso.status, CheckStatus::Ok);
        assert!(iso.detail.contains("acme"));
    }

    #[test]
    fn config_based_compliance_profile_check() {
        let settings = Settings::default();
        let checks = config_based(&settings);
        let cp = checks
            .iter()
            .find(|c| c.name == "compliance_profile")
            .unwrap();
        assert_eq!(cp.status, CheckStatus::Warn);

        let mut settings = Settings::default();
        settings.compliance.insert(
            "acme".to_string(),
            crate::config::compliance::TenantComplianceConfig::default(),
        );
        let checks = config_based(&settings);
        let cp = checks
            .iter()
            .find(|c| c.name == "compliance_profile")
            .unwrap();
        assert_eq!(cp.status, CheckStatus::Ok);
        assert!(cp.detail.contains("1"));
    }
}
