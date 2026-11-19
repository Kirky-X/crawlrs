// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Compliance report aggregation and rendering.
//!
//! Aggregates compliance-relevant signals for a tenant into a five-line text
//! report. Rendering is pure (inputs are counts) so it is verifiable without a
//! database; the caller supplies values from the audit/task repositories.

/// Inputs to a compliance report.
#[derive(Debug, Clone, Default)]
pub struct ComplianceReportData {
    /// Escalations that were blocked on compliance grounds.
    pub compliance_rejects: u64,
    /// Escalations needing manual review (robots-disallowed but escalated).
    pub needs_review: u64,
    /// ai.txt violations observed.
    pub ai_txt_violations: u64,
    /// PII values detected and minimized.
    pub pii_minimized: u64,
    /// Retention-overdue records; `None` = retention not configured.
    pub retention_overdue: Option<u64>,
}

/// Render the report as five text lines.
pub fn render_compliance_report(tenant: &str, since: &str, data: &ComplianceReportData) -> String {
    let retention = match data.retention_overdue {
        Some(count) => count.to_string(),
        None => "未配置 / not configured".to_string(),
    };
    format!(
        "tenant={tenant} since={since}\n\
         合规拒绝 / compliance rejects: {}\n\
         需人工复核 / needs review: {}\n\
         ai.txt 违规 / violations: {}\n\
         PII 检出最小化 / pii minimized: {}\n\
         保留超期 / retention overdue: {retention}",
        data.compliance_rejects, data.needs_review, data.ai_txt_violations, data.pii_minimized,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_report_shows_zero_values_and_unconfigured_retention() {
        let out = render_compliance_report("acme", "30d", &ComplianceReportData::default());
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 6); // header + 5 metric lines
        assert!(out.contains("compliance rejects: 0"));
        assert!(out.contains("not configured"));
    }

    #[test]
    fn configured_report_shows_all_counts() {
        let data = ComplianceReportData {
            compliance_rejects: 2,
            needs_review: 1,
            ai_txt_violations: 3,
            pii_minimized: 7,
            retention_overdue: Some(4),
        };
        let out = render_compliance_report("acme", "30d", &data);
        assert!(out.contains("compliance rejects: 2"));
        assert!(out.contains("needs review: 1"));
        assert!(out.contains("violations: 3"));
        assert!(out.contains("pii minimized: 7"));
        assert!(out.contains("retention overdue: 4"));
    }
}
