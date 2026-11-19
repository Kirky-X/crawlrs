// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Route-decision replay rendering.
//!
//! Turns `antibot_escalation` audit entries into a human-readable, time-ordered
//! chain (initial engine → trigger → escalated-to → risk level) plus a summary
//! hop count. Rendering is pure so it can be verified without a database.

use crate::domain::auth::AuditLogEntry;

/// Field name under `metadata` carrying the escalation payload.
const ESCALATION_KEY: &str = "escalation";

/// Render a replay chain from escalation audit entries (already time-ordered).
///
/// Empty input yields the localized "not found" line; the caller exits with
/// code 0 in that case. Each hop renders one line, followed by a summary line
/// reporting the number of escalation hops.
pub fn render_replay_chain(entries: &[AuditLogEntry], task_id: &str) -> String {
    if entries.is_empty() {
        return format!("未找到 task_id={task_id} 的升级记录 / no escalation records found");
    }

    let mut lines = Vec::with_capacity(entries.len() + 1);
    for (idx, entry) in entries.iter().enumerate() {
        let payload = &entry.metadata[ESCALATION_KEY];
        let initial = payload["initial_engine"].as_str().unwrap_or("?");
        let trigger = payload["trigger"].as_str().unwrap_or("?");
        let escalated = payload["escalated_to"].as_str().unwrap_or("?");
        let risk = payload["risk_level"].as_str().unwrap_or("?");
        lines.push(format!(
            "{}: {} -> {} -> {} [{}]",
            idx + 1,
            initial,
            trigger,
            escalated,
            risk
        ));
    }
    lines.push(format!("汇总 / summary: {} 跳升级 / hops", entries.len()));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::auth::AuditDecision;

    fn escalation_entry(initial: &str, trigger: &str, to: &str, risk: &str) -> AuditLogEntry {
        let mut e = AuditLogEntry {
            id: crate::domain::id::Id::generate(),
            api_key_id: None,
            team_id: None,
            requested_action: "antibot_escalation".to_string(),
            decision: AuditDecision::Allow,
            denial_reason: None,
            scope_used: None,
            ip_address: None,
            trace_id: None,
            user_agent: None,
            request_path: None,
            request_method: None,
            metadata: serde_json::json!({
                "escalation": {
                    "initial_engine": initial,
                    "trigger": trigger,
                    "escalated_to": to,
                    "risk_level": risk
                }
            }),
            created_at: chrono::Utc::now(),
        };
        e.ip_address = None;
        e
    }

    #[test]
    fn empty_entries_render_not_found() {
        let out = render_replay_chain(&[], "task-1");
        assert!(out.contains("未找到"));
        assert!(out.contains("task-1"));
    }

    #[test]
    fn two_hops_render_two_records_and_summary() {
        let entries = vec![
            escalation_entry("http", "antibot_classified", "playwright", "normal"),
            escalation_entry("playwright", "js_challenge", "moli", "high"),
        ];
        let out = render_replay_chain(&entries, "task-1");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3, "2 hop lines + 1 summary line: {out}");
        assert!(lines[0].contains("http -> antibot_classified -> playwright [normal]"));
        assert!(lines[1].contains("playwright -> js_challenge -> moli [high]"));
        assert!(lines[2].contains("2"));
        assert!(lines[2].contains("summary"));
    }

    #[test]
    fn missing_payload_fields_render_placeholder() {
        let mut e = escalation_entry("http", "http403", "playwright", "normal");
        e.metadata = serde_json::json!({"escalation": {"initial_engine": "http"}});
        let out = render_replay_chain(&[e], "t");
        assert!(out.contains("http -> ?"));
    }
}
