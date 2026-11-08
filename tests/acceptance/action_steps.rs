// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE in the project root for full license information.

//! 验收新增动作步骤：POST 模板 / extract sync_wait_ms / map exclude /
//! webhook 按 task id 过滤的投递等待。
//!
//! 独立成文件的原因：`support/mod.rs` 受「mod.rs 不放实现函数」仓库钩子约束，
//! 新步骤拆为 crate 根子模块；cucumber 0.23 的步骤宏经 inventory 在最终二进制
//! 全量收集，模块位置不影响注册与匹配。

use cucumber::when;
use serde_json::Value as Json;

use crate::support::AcceptanceWorld;

/// 展开 `{name}` 模板占位符（与 support::expand_templates 同规则：
/// 先 harness 级 `template_vars`（mock_base/sitemap_base 等站点地址），
/// 后场景级 ctx；后者可覆盖前者）。
async fn expand(w: &AcceptanceWorld, mut s: String) -> String {
    let harness = w.get_harness().await;
    for (name, value) in &harness.template_vars {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    for (name, value) in &w.ctx {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    s
}

/// 带认证头发送 JSON POST 并记录响应（与 support::post_json 同语义）。
async fn post_json_authed(w: &mut AcceptanceWorld, path: &str, payload: &Json) {
    let harness = w.get_harness().await;
    let mut request = harness.server.post(path).json(payload);
    if let Some(key) = &w.auth_key {
        request = request.add_header(
            axum_test::http::header::AUTHORIZATION,
            format!("Bearer {key}"),
        );
    }
    w.last_response = Some(request.await);
}

/// scrape.feature：POST `_cancel` 等动作端点（ctx 模板路径 + 空 JSON 体）。
#[when(expr = "I POST template {string}")]
async fn when_post_template(w: &mut AcceptanceWorld, template: String) {
    let path = expand(w, template).await;
    post_json_authed(w, &path, &serde_json::json!({})).await;
}

/// extract.feature：显式 sync_wait_ms（>30000 走 handler 手动校验 → 400）。
#[when(expr = "I extract url {string} at {string} with sync wait ms {int}")]
async fn when_extract_with_sync_wait(w: &mut AcceptanceWorld, url: String, path: String, ms: i64) {
    let url = expand(w, url).await;
    let payload = serde_json::json!({"urls": [url], "prompt": "test", "sync_wait_ms": ms});
    post_json_authed(w, &path, &payload).await;
}

/// map.feature：exclude 通配过滤（契约先 include 后 exclude；本场景只给 exclude）。
#[when(expr = "I map {string} excluding {string}")]
async fn when_map_excluding(w: &mut AcceptanceWorld, url: String, exclude: String) {
    let url = expand(w, url).await;
    let payload = serde_json::json!({"url": url, "exclude_patterns": [exclude]});
    post_json_authed(w, "/v1/map", &payload).await;
}

/// platform.feature：`POST /v1/tasks/_cancel` 批量取消——task_ids 取自两个
/// ctx 键（TaskCancelRequestDto 契约：`task_ids` 必填非空；响应
/// `data.cancelled_tasks` 数组 + `data.total_cancelled` 计数）。
#[when(expr = "I cancel tasks from context {string} and {string} at {string}")]
async fn when_cancel_tasks_from_context(
    w: &mut AcceptanceWorld,
    first: String,
    second: String,
    path: String,
) {
    let read = |key: &str| {
        w.ctx
            .get(key)
            .cloned()
            .unwrap_or_else(|| panic!("ctx key {key} missing"))
    };
    let payload = serde_json::json!({"task_ids": [read(&first), read(&second)]});
    post_json_authed(w, &path, &payload).await;
}

/// webhook.feature：等待投递且响应体包含 ctx 指定的 task id。
///
/// mock 接收站跨场景累积投递（garrison 单例约束下进程级共享），support 的
/// `wait_for_webhook_delivery` 取首条匹配即返回——前一场景的历史投递会抢先把
/// `last_delivery` 占位，导致任务 id 断言偶发失败。本步骤按 task id 过滤，
/// 只接受属于当前场景任务的投递，消除该跨场景串扰。
#[when(
    expr = "I wait at most {int} seconds for a webhook delivery to {string} for task from context {string}"
)]
async fn when_wait_webhook_delivery_for_task(
    w: &mut AcceptanceWorld,
    max_secs: u64,
    path_prefix: String,
    ctx_name: String,
) {
    let harness = w.get_harness().await;
    let task_id = w
        .ctx
        .get(&ctx_name)
        .cloned()
        .unwrap_or_else(|| panic!("ctx key {ctx_name} missing"));
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(max_secs);
    loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "no webhook delivery to {path_prefix} containing task {task_id} within {max_secs}s"
        );
        if let Some(requests) = harness.mock_main.received_requests().await {
            for request in &requests {
                let path = request.url.path().to_string();
                if request.method == wiremock::http::Method::POST && path.starts_with(&path_prefix)
                {
                    let body = String::from_utf8_lossy(&request.body).to_string();
                    if body.contains(task_id.as_str()) {
                        let headers = request
                            .headers
                            .iter()
                            .map(|(name, value)| {
                                (
                                    name.as_str().to_ascii_lowercase(),
                                    value.to_str().unwrap_or_default().to_string(),
                                )
                            })
                            .collect();
                        w.last_delivery = Some(crate::support::CapturedDelivery { headers, body });
                        return;
                    }
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}
