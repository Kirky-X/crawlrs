// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 顺序路由·反爬挑战处理 — 从 `route_sequential.rs` 拆分（仅 `content` 特性）

use super::super::EngineRouter;
use crate::engines::engine_client::InternalScrapeResponse;
use crate::engines::router::check_antibot_response;
use crate::engines::session::SessionIdentity;
use crate::utils::host_discipline::ObserveOutcome;
use log::warn;
use std::sync::Arc;
use std::time::Duration;

/// 已知 WAF 变体：403 命中任一即为带 challenge 的拦截页，
/// 只有"无 WAF 指纹的裸 403"才计入 host 纪律连击
#[cfg(feature = "content")]
fn is_known_waf(tech: crate::engines::antibot::AntiBotTech) -> bool {
    use crate::engines::antibot::AntiBotTech;
    matches!(
        tech,
        AntiBotTech::Cloudflare
            | AntiBotTech::Akamai
            | AntiBotTech::PerimeterX
            | AntiBotTech::DataDome
            | AntiBotTech::Imperva
            | AntiBotTech::Sucuri
            | AntiBotTech::Kasada
            | AntiBotTech::AwsWaf
    )
}

#[cfg(feature = "content")]
impl EngineRouter {
    /// 反爬挑战检测与处理（route_internal 成功臂挑战分支，原样提取）
    ///
    /// 命中 `needs_browser` 时依次完成：F7 host 反爬记忆、host 纪律观测
    /// （429 → 冷却；裸 403 无 WAF 指纹 → 连击计数）、引擎失败统计、
    /// F2 身份 mark_bad、失败指标、F4 失败现场快照，然后返回
    /// `Err(challenge_reason)`（调用方簿记 last_error/force_needs_js/tracker）；
    /// 未命中时原样返回 `Ok(response)`。
    pub(super) async fn record_antibot_failure(
        &self,
        response: InternalScrapeResponse,
        request_url: &str,
        engine_name: &str,
        response_time: Duration,
        session_identity: Option<&Arc<SessionIdentity>>,
    ) -> Result<InternalScrapeResponse, String> {
        if let Some(detection) = check_antibot_response(&response, request_url) {
            if detection.needs_browser {
                // F7：记录 host 反爬记忆，TTL 内后续同 host
                // 请求直接按 needs_js 路由
                if let Ok(u) = url::Url::parse(request_url) {
                    if let Some(host) = u.host_str() {
                        self.record_host_browser_hint(host);
                        // host 纪律观测：429 → 冷却；
                        // 裸 403（无 WAF 指纹）→ 连击计数
                        if detection.tech == crate::engines::antibot::AntiBotTech::RateLimited {
                            self.host_discipline.observe(
                                host,
                                ObserveOutcome::RateLimited {
                                    retry_after: detection
                                        .retry_after_secs
                                        .map(Duration::from_secs),
                                },
                            );
                        } else if response.status_code == 403 && !is_known_waf(detection.tech) {
                            self.host_discipline
                                .observe(host, ObserveOutcome::Forbidden { challenge: false });
                        }
                    }
                }
                self.update_engine_stats(engine_name, false, response_time);
                // F2：挑战页 = 身份可疑证据，同址 mark_bad
                if let Some(identity) = session_identity {
                    self.session.mark_bad(identity);
                }
                self.metrics
                    .record_engine_failure(engine_name, &detection.reason);
                // F4：失败现场快照（签名键去重存档，旁路观测；
                // 内部 spawn_blocking 落盘，不阻塞 worker 线程）
                self.snapshot_failure(
                    request_url,
                    response.status_code,
                    &detection.reason,
                    response,
                )
                .await;
                warn!(
                    "Engine {} returned anti-bot challenge ({:?}): {}, \
                                     forcing needs_js for subsequent attempts",
                    engine_name, detection.tech, detection.reason
                );
                return Err(detection.reason);
            }
        }
        Ok(response)
    }
}

#[cfg(feature = "content")]
impl EngineRouter {
    /// F4：失败现场快照（旁路观测，永不失败）
    ///
    /// 快照器未配置（`failure_snapshot_dir` 空串）时 `Option` 短路零成本；
    /// 内部 IO 失败已在 [`FailureSnapshotter::snapshot`](crate::infrastructure::observability::failure_snapshot::FailureSnapshotter::snapshot)
    /// 内 `log::debug` 静默降级，绝不影响抓取主流程。
    /// 仅透传引擎已产出的 screenshot（不额外触发截图）。
    #[cfg(feature = "content")]
    pub(super) async fn snapshot_failure(
        &self,
        url: &str,
        status_code: u16,
        reason: &str,
        response: InternalScrapeResponse,
    ) {
        if let Some(snapshotter) = &self.failure_snapshot {
            // 按值接收：content/screenshot 直接移入快照输入，避免 async 线程上的整份拷贝
            let InternalScrapeResponse {
                content,
                screenshot,
                headers,
                ..
            } = response;
            snapshotter
                .snapshot(
                    crate::infrastructure::observability::failure_snapshot::SnapshotInput {
                        url,
                        status_code,
                        reason,
                        content,
                        headers: &headers,
                        screenshot,
                    },
                )
                .await;
        }
    }
}

#[cfg(all(test, feature = "content"))]
mod snapshot_failure_tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;

    fn sample_response() -> InternalScrapeResponse {
        InternalScrapeResponse {
            js_degraded: false,
            captured_responses: Vec::new(),
            status_code: 403,
            content: "<html>challenge</html>".to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 42,
            final_url: None,
        }
    }

    fn dir_file_count(dir: &std::path::Path) -> usize {
        fs::read_dir(dir).expect("read_dir").count()
    }

    /// 快照器为 None（默认未配置）时调用点零开销短路（不 panic、无落盘）
    #[tokio::test]
    async fn snapshot_failure_none_short_circuits_without_panic() {
        let router = EngineRouter::new(Vec::new());
        assert!(router.failure_snapshot.is_none(), "default must be None");
        router
            .snapshot_failure("https://x.com/a", 403, "waf", sample_response())
            .await;
    }

    /// 注入快照器后 snapshot_failure 落盘 .html + .meta.json
    #[tokio::test]
    async fn snapshot_failure_with_snapshotter_writes_archive() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("snapshots");
        let mut router = EngineRouter::new(Vec::new());
        router.set_failure_snapshotter(Some(Arc::new(
            crate::infrastructure::observability::failure_snapshot::FailureSnapshotter::new(
                dir.clone(),
                200,
            ),
        )));
        router
            .snapshot_failure("https://x.com/a", 403, "waf", sample_response())
            .await;
        assert_eq!(dir_file_count(&dir), 2, "html + meta.json");
    }
}
