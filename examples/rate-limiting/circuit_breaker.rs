// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 熔断器模式示例
//!
//! 演示 crawlrs 的 `CircuitBreaker` 熔断器实现：
//! - 三种状态：Closed（正常）→ Open（熔断）→ HalfOpen（探测恢复）
//! - 按引擎粒度独立熔断
//! - 自定义熔断配置
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example circuit_breaker
//! ```

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 熔断器模式示例");
    info!("=====================================\n");

    // 1. 熔断器概念
    info!("1️⃣  熔断器状态机");
    info!("-----------------------------");
    info!("");
    info!("  Closed ──失败次数达阈值──→ Open");
    info!("    ↑                           │");
    info!("    │                     恢复超时后");
    info!("    │                           ↓");
    info!("    └──探测成功──── HalfOpen ────┘");
    info!("                     │");
    info!("               探测失败 → 回到 Open");
    info!("");

    // 2. 配置说明
    info!("2️⃣  熔断器配置");
    info!("-----------------------------");
    info!("  CircuitConfig {{");
    info!("    failure_threshold: 5,       // 连续失败 5 次触发熔断");
    info!("    recovery_timeout: 30s,      // 30s 后进入半开状态");
    info!("    failure_window: 60s,        // 60s 内的失败计数");
    info!("  }}");
    info!("");
    info!("  默认配置适合大多数场景:");
    info!("  let breaker = CircuitBreaker::new();");
    info!("");
    info!("  自定义默认配置:");
    info!("  let breaker = CircuitBreaker::with_default_config(CircuitConfig {{");
    info!("    failure_threshold: 3,");
    info!("    recovery_timeout: Duration::from_secs(60),");
    info!("    failure_window: Duration::from_secs(120),");
    info!("  }});");
    info!("");

    // 3. 按引擎配置
    info!("3️⃣  按引擎独立配置");
    info!("-----------------------------");
    info!("  可为不同引擎设置不同的熔断阈值:");
    info!("");
    info!("  breaker.set_config(\"reqwest\", CircuitConfig {{");
    info!("    failure_threshold: 10,  // HTTP 引擎容忍更多失败");
    info!("    recovery_timeout: Duration::from_secs(30),");
    info!("    failure_window: Duration::from_secs(60),");
    info!("  }});");
    info!("");
    info!("  breaker.set_config(\"playwright\", CircuitConfig {{");
    info!("    failure_threshold: 3,   // 浏览器引擎更敏感");
    info!("    recovery_timeout: Duration::from_secs(60),");
    info!("    failure_window: Duration::from_secs(120),");
    info!("  }});");
    info!("");

    // 4. 工作流程
    info!("4️⃣  工作流程");
    info!("-----------------------------");
    info!("  EngineRouter 在每次请求前后自动操作熔断器:");
    info!("");
    info!("  1. 选择引擎前: breaker.is_open(\"engine_name\")");
    info!("     → Open 状态: 跳过该引擎，选择下一个");
    info!("     → Closed/HalfOpen: 正常尝试");
    info!("");
    info!("  2. 请求成功: breaker.record_success(\"engine_name\")");
    info!("     → 重置失败计数");
    info!("     → HalfOpen → Closed（恢复）");
    info!("");
    info!("  3. 请求失败: breaker.record_failure(\"engine_name\")");
    info!("     → 累加失败计数");
    info!("     → 达到阈值 → Closed → Open（熔断）");
    info!("");
    info!("  💡 熔断器确保:");
    info!("     - 故障引擎不会持续收到请求");
    info!("     - 系统自动降级到健康引擎");
    info!("     - 故障恢复后自动恢复流量");

    info!("\n=====================================");
    info!("✨ 熔断器模式示例完成");
}
