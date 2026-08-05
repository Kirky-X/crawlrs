// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 任务管理 API 示例
//!
//! 演示 crawlrs 的任务管理端点：
//! - `POST /v1/tasks/_query` — 批量查询任务状态
//! - `POST /v1/tasks/_cancel` — 批量取消任务
//! - 同步等待机制（`sync_wait_ms`）
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example task_management
//! ```

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 任务管理 API 示例");
    info!("=====================================\n");

    // 1. 任务查询 API
    info!("1️⃣  任务查询 (POST /v1/tasks/_query)");
    info!("-----------------------------");
    info!("  请求体 (TaskQueryRequestDto):");
    info!("  {{");
    info!("    \"task_ids\": [\"<uuid-1>\", \"<uuid-2>\"],");
    info!("    \"team_id\": \"<team-uuid>\",");
    info!("    \"task_types\": [\"scrape\", \"crawl\"],");
    info!("    \"statuses\": [\"running\", \"completed\"],");
    info!("    \"created_after\": \"2025-01-01T00:00:00Z\",");
    info!("    \"created_before\": \"2025-12-31T23:59:59Z\",");
    info!("    \"crawl_id\": \"<crawl-uuid>\",");
    info!("    \"page\": 1,");
    info!("    \"page_size\": 20,");
    info!("    \"sync_wait_ms\": 5000,");
    info!("    \"include_results\": true");
    info!("  }}");
    info!("");
    info!("  过滤维度:");
    info!("    - task_ids: 按任务 ID 批量查询（可选）");
    info!("    - task_types: 按类型过滤（scrape / crawl）");
    info!("    - statuses: 按状态过滤（pending / running / completed / failed / cancelled）");
    info!("    - created_after / created_before: 按创建时间范围过滤");
    info!("    - crawl_id: 查询特定整站爬取任务的所有子任务");
    info!("");
    info!("  同步等待:");
    info!("    - sync_wait_ms > 0 时，服务端会轮询等待任务完成");
    info!("    - 超时后仍返回当前状态（不会报错）");
    info!("    - 最大 30000ms（30 秒）");
    info!("");
    info!("  include_results:");
    info!("    - 为 true 时，响应中包含已爬取的页面结果数据");
    info!("    - 仅对 completed 状态的 scrape 任务有效");
    info!("");

    // 2. 任务取消 API
    info!("2️⃣  任务取消 (POST /v1/tasks/_cancel)");
    info!("-----------------------------");
    info!("  请求体 (TaskCancelRequestDto):");
    info!("  {{");
    info!("    \"task_ids\": [\"<uuid-1>\", \"<uuid-2>\"],");
    info!("    \"team_id\": \"<team-uuid>\",");
    info!("    \"force\": false,");
    info!("    \"sync_wait_ms\": 5000");
    info!("  }}");
    info!("");
    info!("  参数说明:");
    info!("    - task_ids: 要取消的任务 ID 列表（必填，支持批量）");
    info!("    - team_id: 团队 ID（必填，必须与任务所属团队匹配）");
    info!("    - force: 是否强制取消正在执行中的任务（默认 false）");
    info!("    - sync_wait_ms: 同步等待取消完成的时长（默认 5000ms）");
    info!("");
    info!("  响应体 (TaskCancelDataDto):");
    info!("  {{");
    info!("    \"total_cancelled\": 2,");
    info!("    \"total_failed\": 0,");
    info!("    \"cancelled_tasks\": [");
    info!(
        "      {{ \"task_id\": \"<uuid>\", \"status\": \"cancelled\", \"cancelled_at\": \"...\" }}"
    );
    info!("    ],");
    info!("    \"failed_tasks\": []");
    info!("  }}");
    info!("");

    // 3. 单个任务取消
    info!("3️⃣  单个任务取消端点");
    info!("-----------------------------");
    info!("  取消单个 scrape 任务:");
    info!("    POST /v1/scrape/{{id}}/_cancel");
    info!("    → 返回 200 OK + 取消确认消息");
    info!("");
    info!("  取消单个 crawl 任务:");
    info!("    POST /v1/crawl/{{id}}/_cancel");
    info!("    → 返回 200 OK + 取消确认消息");
    info!("");
    info!("  权限校验:");
    info!("    - 只能取消自己团队的任务（team_id 匹配）");
    info!("    - 任务不存在返回 404");
    info!("    - 非本团队任务返回 403");
    info!("");

    // 4. 使用场景
    info!("4️⃣  常见使用场景");
    info!("-----------------------------");
    info!("");
    info!("  📝 场景 1: 查询所有运行中的任务");
    info!("    POST /v1/tasks/_query");
    info!("    {{ \"team_id\": \"...\", \"statuses\": [\"running\"], \"page_size\": 50 }}");
    info!("");
    info!("  📝 场景 2: 查询特定爬取任务的所有子任务");
    info!("    POST /v1/tasks/_query");
    info!("    {{ \"team_id\": \"...\", \"crawl_id\": \"<crawl-uuid>\" }}");
    info!("");
    info!("  📝 场景 3: 批量取消超时任务");
    info!("    POST /v1/tasks/_cancel");
    info!("    {{ \"task_ids\": [\"...\", \"...\"], \"team_id\": \"...\", \"force\": true }}");
    info!("");
    info!("  📝 场景 4: 同步等待任务完成后获取结果");
    info!("    POST /v1/tasks/_query");
    info!("    {{ \"task_ids\": [\"...\"], \"team_id\": \"...\", \"sync_wait_ms\": 10000, \"include_results\": true }}");
    info!("");

    info!("  💡 最佳实践:");
    info!("     - 使用 sync_wait_ms 避免频繁轮询");
    info!("     - 批量查询时合理设置 page_size（默认 20，最大 100）");
    info!("     - 取消操作使用 force=true 确保正在执行的任务也被终止");
    info!("     - 定期清理已完成的旧任务数据");

    info!("\n=====================================");
    info!("✨ 任务管理 API 示例完成");
}
