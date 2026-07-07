// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 爬取状态跟踪示例
//!
//! 演示如何监控爬取任务的执行状态和进度。
//!
//! # 状态类型
//!
//! - **pending**: 等待执行
//! - **running**: 正在执行
//! - **completed**: 已完成
//! - **failed**: 失败
//! - **cancelled**: 已取消
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example status_tracking
//!

use log::info;

/// 爬取任务状态
#[derive(Debug, Clone, PartialEq)]
enum CrawlStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

/// 爬取进度信息
#[derive(Debug)]
struct CrawlProgress {
    status: CrawlStatus,
    total_pages: u32,
    completed_pages: u32,
    failed_pages: u32,
    queued_pages: u32,
    start_time: std::time::SystemTime,
    elapsed_seconds: u64,
    pages_per_second: f64,
}

impl CrawlProgress {
    fn new() -> Self {
        Self {
            status: CrawlStatus::Pending,
            total_pages: 0,
            completed_pages: 0,
            failed_pages: 0,
            queued_pages: 0,
            start_time: std::time::SystemTime::now(),
            elapsed_seconds: 0,
            pages_per_second: 0.0,
        }
    }

    fn update(&mut self) {
        if let Ok(elapsed) = self.start_time.elapsed() {
            self.elapsed_seconds = elapsed.as_secs();
        }

        self.queued_pages = self
            .total_pages
            .saturating_sub(self.completed_pages + self.failed_pages);

        if self.elapsed_seconds > 0 {
            self.pages_per_second = self.completed_pages as f64 / self.elapsed_seconds as f64;
        }
    }

    fn display(&self) {
        let status_str = match &self.status {
            CrawlStatus::Pending => "⏳ 等待中".to_string(),
            CrawlStatus::Running => "🔄 运行中".to_string(),
            CrawlStatus::Completed => "✅ 完成".to_string(),
            CrawlStatus::Failed(e) => format!("❌ 失败: {}", e),
            CrawlStatus::Cancelled => "🛑 已取消".to_string(),
        };

        info!("  状态: {}", status_str);
        info!(
            "  进度: {}/{} ({:.1}%)",
            self.completed_pages,
            self.total_pages,
            if self.total_pages > 0 {
                self.completed_pages as f64 / self.total_pages as f64 * 100.0
            } else {
                0.0
            }
        );
        info!(
            "  成功: {} | 失败: {} | 排队: {}",
            self.completed_pages, self.failed_pages, self.queued_pages
        );
        info!(
            "  耗时: {}秒 | 速度: {:.2}页/秒",
            self.elapsed_seconds, self.pages_per_second
        );
    }
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始爬取状态跟踪示例");
    info!("=====================================\n");

    // 1. 状态类型说明
    info!("1️⃣  状态类型说明");
    info!("-----------------------------");
    info!("");
    info!("📋 爬取任务状态:");
    info!("   ⏳ pending  - 任务已创建，等待调度执行");
    info!("   🔄 running  - 任务正在执行，正在爬取页面");
    info!("   ✅ completed - 任务已成功完成");
    info!("   ❌ failed    - 任务执行失败");
    info!("   🛑 cancelled - 任务被用户取消");
    info!("");

    // 2. 状态转换演示
    info!("2️⃣  状态转换演示");
    info!("-----------------------------");

    let mut progress = CrawlProgress::new();

    // 初始状态
    info!("📝 任务创建 - 初始状态:");
    progress.display();
    info!("");

    // 开始执行
    info!("🚀 任务开始执行:");
    progress.status = CrawlStatus::Running;
    progress.total_pages = 100;
    progress.completed_pages = 0;
    progress.update();
    progress.display();
    info!("");

    // 模拟爬取过程
    info!("🔄 模拟爬取过程:");
    for i in (0..=100).step_by(10) {
        progress.completed_pages = i;
        let failed = (i as f64 * 0.05) as u32;
        progress.failed_pages = failed;
        progress.update();

        let progress_bar = format!(
            "[{}{}]",
            "█".repeat(i as usize / 10),
            "░".repeat(10 - i as usize / 10)
        );
        info!("  {} {}%", progress_bar, i);

        // 模拟延迟
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    info!("");

    // 完成状态
    info!("✅ 任务完成:");
    progress.status = CrawlStatus::Completed;
    progress.update();
    progress.display();
    info!("");

    // 3. 进度监控示例
    info!("3️⃣  实时进度监控");
    info!("-----------------------------");

    info!("📊 监控指标:");
    info!("   - 总页面数");
    info!("   - 已完成页面数");
    info!("   - 失败页面数");
    info!("   - 排队页面数");
    info!("   - 已用时间");
    info!("   - 平均速度（页/秒）");
    info!("   - 预计剩余时间");
    info!("");

    // 4. 错误处理
    info!("4️⃣  错误状态处理");
    info!("-----------------------------");

    let mut error_progress = CrawlProgress::new();
    error_progress.status = CrawlStatus::Running;
    error_progress.total_pages = 50;
    error_progress.completed_pages = 20;
    error_progress.failed_pages = 5;
    error_progress.update();

    info!("📝 任务执行中:");
    error_progress.display();
    info!("");

    // 模拟失败
    info!("⚠️  遇到错误，任务失败:");
    error_progress.status = CrawlStatus::Failed("Connection timeout after 3 retries".to_string());
    error_progress.update();
    error_progress.display();
    info!("");

    info!("💡 错误处理建议:");
    info!("   - 记录详细的错误信息");
    info!("   - 提供错误分类（网络、服务器、解析等）");
    info!("   - 支持重试机制");
    info!("   - 保留已爬取的数据");

    info!("\n=====================================");
    info!("✨ 爬取状态跟踪示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - 定期更新进度信息");
    info!("   - 提供取消任务的接口");
    info!("   - 记录详细的错误日志");
    info!("   - 支持进度恢复");
}
