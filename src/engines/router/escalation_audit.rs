// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Anti-bot escalation audit events.
//!
//! Engine escalation paths (anti-bot classified re-route, JS challenge probe,
//! sequential fallback) were previously silent routing decisions. This module
//! turns each escalation hop into a structured `antibot_escalation` audit
//! entry reusing `AuditLogEntry.metadata` (no schema change), and correlates
//! the hop with the robots.txt verdict for the target host (high-risk flag
//! when a `Disallowed` host still escalates).
//!
//! Compiles unconditionally (zero new deps); `emit_escalation` with `None`
//! audit sink is a zero-cost no-op, so agent-lib builds are unaffected.

use serde::Serialize;
use std::sync::Arc;

/// Why an escalation hop happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationTrigger {
    /// `antibot::classify` detected a challenge needing a browser engine.
    AntibotClassified,
    /// JS upgrade probe verdict `upgrade=true` (SPA shell).
    JsChallenge,
    /// Fallback after an HTTP 403.
    Http403,
    /// Fallback after an HTTP 429.
    Http429,
    /// Fallback after a TLS fingerprint rejection.
    TlsFingerprintRejected,
    /// Sequential fallback engine switch (unclassified).
    SequentialFallback,
}

/// Robots.txt verdict for the escalated target (cache-read only, no network).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RobotsVerdict {
    /// robots.txt cached and crawling allowed.
    Allowed,
    /// robots.txt cached and crawling disallowed (high risk when escalating).
    Disallowed,
    /// No robots.txt judgment available (not configured or not cached).
    NotFetched,
}

/// One escalation hop (initial engine → escalated engine).
#[derive(Debug, Clone)]
pub struct EscalationEvent {
    /// Tenant identifier (router-level context, currently optional).
    pub tenant_id: Option<String>,
    /// Owning team (router-level context, currently optional).
    pub team_id: Option<crate::domain::id::Id>,
    /// Crawl task this hop belongs to (router-level context, optional).
    pub task_id: Option<crate::domain::id::Id>,
    /// Target host being scraped.
    pub target_host: String,
    /// Engine the hop started from (engine `name()` constant).
    pub initial_engine: String,
    /// Engine the hop escalated to.
    pub escalated_to: String,
    /// Why the hop happened.
    pub trigger: EscalationTrigger,
    /// Robots.txt verdict correlated at emit time.
    pub robots_verdict: RobotsVerdict,
}

impl EscalationEvent {
    /// Risk level: `high` when a robots-disallowed host still escalates,
    /// `normal` otherwise.
    pub fn risk_level(&self) -> &'static str {
        if self.robots_verdict == RobotsVerdict::Disallowed {
            "high"
        } else {
            "normal"
        }
    }

    /// Serialize the payload carried in `AuditLogEntry.metadata`.
    fn payload(&self) -> serde_json::Value {
        serde_json::json!({
            "tenant_id": self.tenant_id,
            "team_id": self.team_id.map(|id| id.to_string()),
            "task_id": self.task_id.map(|id| id.to_string()),
            "target_host": self.target_host,
            "initial_engine": self.initial_engine,
            "escalated_to": self.escalated_to,
            "trigger": self.trigger,
            "robots_txt_verdict": self.robots_verdict,
            "risk_level": self.risk_level(),
        })
    }

    /// Build the audit entry (`requested_action = "antibot_escalation"`).
    pub fn into_entry(self) -> crate::domain::auth::AuditLogEntry {
        crate::domain::services::audit_log_builder::AuditLogBuilder::new(
            "antibot_escalation",
            crate::domain::auth::AuditDecision::Allow,
        )
        .with_metadata("escalation", self.payload())
        .build()
    }
}

/// Resolve the robots verdict for an escalation target (T005).
///
/// Read-only cache lookup on the checker (`cached_verdict`, zero network);
/// missing checker or uncached host yields `NotFetched`.
pub fn resolve_robots_verdict(
    checker: Option<&crate::utils::robots::RobotsChecker>,
    target_url: &str,
) -> RobotsVerdict {
    let Some(checker) = checker else {
        return RobotsVerdict::NotFetched;
    };
    match checker.cached_verdict(target_url) {
        Some(true) => RobotsVerdict::Allowed,
        Some(false) => RobotsVerdict::Disallowed,
        None => RobotsVerdict::NotFetched,
    }
}

/// Attribute a sequential fallback hop from the failing engine error (T004).
///
/// HTTP engines return 4xx/5xx bodies as successful responses (the antibot
/// classifier handles those), so the 403/429 attribution reads the
/// `AntiBotDetected` reason string produced by the classifier; TLS fingerprint
/// rejections surface as message-level "tls" markers. Anything else is a plain
/// sequential fallback.
pub fn fallback_trigger(
    err: Option<&crate::engines::engine_client::EngineError>,
) -> EscalationTrigger {
    use crate::engines::engine_client::EngineError;
    let Some(err) = err else {
        return EscalationTrigger::SequentialFallback;
    };
    match err {
        EngineError::AntiBotDetected(reason) if reason.contains("429") => {
            EscalationTrigger::Http429
        }
        EngineError::AntiBotDetected(reason) if reason.contains("403") => {
            EscalationTrigger::Http403
        }
        EngineError::RequestFailed(m) | EngineError::Other(m) | EngineError::Internal(m)
            if m.to_ascii_lowercase().contains("tls") =>
        {
            EscalationTrigger::TlsFingerprintRejected
        }
        _ => EscalationTrigger::SequentialFallback,
    }
}

/// Emit one escalation event through the audit sink (fire-and-forget).
///
/// `None` sink returns immediately (zero-cost bypass for agent-lib / audit-less
/// deployments); `Some` spawns the async write off the routing hot path
/// (mirrors the garrison listener's async bridging pattern).
pub fn emit_escalation(
    audit: Option<&Arc<dyn crate::domain::services::audit_service::AuditServiceTrait>>,
    ev: EscalationEvent,
) {
    let Some(audit) = audit else {
        return;
    };
    let audit = audit.clone();
    let entry = ev.into_entry();
    tokio::spawn(async move {
        if let Err(e) = audit.log(entry).await {
            log::warn!("antibot_escalation audit write failed: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event(robots: RobotsVerdict) -> EscalationEvent {
        EscalationEvent {
            tenant_id: Some("tenant-1".to_string()),
            team_id: Some(crate::domain::id::Id::generate()),
            task_id: Some(crate::domain::id::Id::generate()),
            target_host: "example.com".to_string(),
            initial_engine: "http".to_string(),
            escalated_to: "playwright".to_string(),
            trigger: EscalationTrigger::AntibotClassified,
            robots_verdict: robots,
        }
    }

    /// Disallowed + escalation → high risk.
    #[test]
    fn risk_level_high_when_robots_disallowed() {
        assert_eq!(sample_event(RobotsVerdict::Disallowed).risk_level(), "high");
    }

    /// Allowed (and NotFetched) → normal risk.
    #[test]
    fn risk_level_normal_when_allowed_or_not_fetched() {
        assert_eq!(sample_event(RobotsVerdict::Allowed).risk_level(), "normal");
        assert_eq!(
            sample_event(RobotsVerdict::NotFetched).risk_level(),
            "normal"
        );
    }

    /// `into_entry` metadata carries all payload keys under `escalation`.
    #[test]
    fn into_entry_metadata_contains_all_payload_keys() {
        let entry = sample_event(RobotsVerdict::Disallowed).into_entry();
        assert_eq!(entry.requested_action, "antibot_escalation");
        let payload = &entry.metadata["escalation"];
        for key in [
            "tenant_id",
            "team_id",
            "task_id",
            "target_host",
            "initial_engine",
            "escalated_to",
            "trigger",
            "robots_txt_verdict",
            "risk_level",
        ] {
            assert!(
                payload.get(key).is_some(),
                "metadata.escalation missing key '{key}': {payload}"
            );
        }
        assert_eq!(payload["risk_level"], "high");
        assert_eq!(payload["trigger"], "antibot_classified");
        assert_eq!(payload["robots_txt_verdict"], "disallowed");
    }

    /// `emit_escalation(None, ..)` must not panic (audit-less bypass).
    #[tokio::test]
    async fn emit_escalation_none_sink_is_noop() {
        emit_escalation(None, sample_event(RobotsVerdict::Allowed));
    }

    /// Mock audit sink recording `log()` calls.
    struct MockAudit {
        actions: std::sync::Mutex<Vec<String>>,
    }

    impl MockAudit {
        fn new() -> Self {
            Self {
                actions: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn log_count(&self) -> usize {
            self.actions.lock().unwrap().len()
        }
    }

    #[async_trait::async_trait]
    impl crate::domain::services::audit_service::AuditServiceTrait for MockAudit {
        async fn log(
            &self,
            entry: crate::domain::auth::AuditLogEntry,
        ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
            self.actions.lock().unwrap().push(entry.requested_action);
            Ok(())
        }

        async fn log_allow(
            &self,
            _action: String,
            _api_key_id: crate::domain::id::Id,
            _team_id: crate::domain::id::Id,
            _scope: crate::domain::auth::ApiKeyScope,
        ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
            Ok(())
        }

        async fn log_deny(
            &self,
            _action: String,
            _api_key_id: Option<crate::domain::id::Id>,
            _team_id: Option<crate::domain::id::Id>,
            _reason: String,
            _scope: Option<crate::domain::auth::ApiKeyScope>,
        ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
            Ok(())
        }

        async fn get_logs_for_key(
            &self,
            _api_key_id: crate::domain::id::Id,
            _limit: u64,
            _offset: u64,
        ) -> Result<
            Vec<crate::domain::auth::AuditLogEntry>,
            crate::domain::services::audit_service::AuditServiceError,
        > {
            Ok(Vec::new())
        }

        async fn get_logs_for_team(
            &self,
            _team_id: crate::domain::id::Id,
            _limit: u64,
            _offset: u64,
        ) -> Result<
            Vec<crate::domain::auth::AuditLogEntry>,
            crate::domain::services::audit_service::AuditServiceError,
        > {
            Ok(Vec::new())
        }

        async fn get_denied_requests(
            &self,
            _api_key_id: crate::domain::id::Id,
            _limit: u64,
        ) -> Result<
            Vec<crate::domain::auth::AuditLogEntry>,
            crate::domain::services::audit_service::AuditServiceError,
        > {
            Ok(Vec::new())
        }
    }

    // ===== T005: robots verdict correlation =====

    fn test_checker() -> crate::utils::robots::RobotsChecker {
        let settings = crate::config::settings::RobotsSettings::default();
        crate::utils::robots::RobotsChecker::new(
            std::sync::Arc::new(reqwest::Client::new()),
            &settings,
            None,
            None,
        )
    }

    /// Missing checker → NotFetched.
    #[test]
    fn resolve_robots_verdict_without_checker_is_not_fetched() {
        assert_eq!(
            resolve_robots_verdict(None, "https://example.com/a"),
            RobotsVerdict::NotFetched
        );
    }

    /// Cached Disallow rule → Disallowed; uncached host → NotFetched;
    /// disallowed + escalation → risk_level high (end to end).
    #[tokio::test]
    async fn resolve_robots_verdict_disallowed_cache_marks_high_risk() {
        let checker = test_checker();
        checker
            .seed_memory_cache_for_test(
                "https://blocked.example.com/a",
                "User-agent: *\nDisallow: /\n",
            )
            .await;
        checker
            .seed_memory_cache_for_test("https://open.example.com/a", "User-agent: *\nDisallow: \n")
            .await;

        assert_eq!(
            resolve_robots_verdict(Some(&checker), "https://blocked.example.com/page"),
            RobotsVerdict::Disallowed
        );
        assert_eq!(
            resolve_robots_verdict(Some(&checker), "https://open.example.com/page"),
            RobotsVerdict::Allowed
        );
        assert_eq!(
            resolve_robots_verdict(Some(&checker), "https://uncached.example.com/page"),
            RobotsVerdict::NotFetched
        );

        // End-to-end: a Disallowed host that still escalates is high risk.
        let mut ev = sample_event(RobotsVerdict::NotFetched);
        ev.robots_verdict =
            resolve_robots_verdict(Some(&checker), "https://blocked.example.com/page");
        assert_eq!(ev.risk_level(), "high");
    }

    /// Some(sink) writes exactly one `antibot_escalation` entry via spawn.
    #[tokio::test]
    async fn emit_escalation_some_sink_writes_one_entry() {
        let mock = Arc::new(MockAudit::new());
        let audit: Arc<dyn crate::domain::services::audit_service::AuditServiceTrait> =
            mock.clone();
        emit_escalation(Some(&audit), sample_event(RobotsVerdict::Allowed));

        // The write is spawned; poll briefly until it lands (bounded wait).
        for _ in 0..100 {
            if mock.log_count() == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(mock.log_count(), 1, "spawned write must land");
        assert_eq!(mock.actions.lock().unwrap()[0], "antibot_escalation");
    }
}
