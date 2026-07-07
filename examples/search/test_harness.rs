// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 搜索测试工具模块
//!
//! 提供搜索测试的通用工具函数和宏，减少重复代码
//!
//! # 使用示例
//!
//! ```rust
//! use crate::test_harness::SearchTestHarness;
//!
//! let harness = SearchTestHarness::new("Google", 60, 10);
//! let result = harness.run_test("test query").await;
//! ```

use crate::utils::search_test::{run_engine_test_with_output, TestResult};
use anyhow::Result;
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use log::{info, Level, Metadata, Record};

/// 最小化的 stderr 日志实现（替代 tracing_subscriber 用于测试）
struct StderrLogger;
impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }
    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{} {}] {}", record.level(), record.target(), record.args());
        }
    }
    fn flush(&self) {}
}

/// 搜索测试工具类
pub struct SearchTestHarness {
    engine_name: String,
    timeout_secs: u64,
    result_limit: u32,
    test_keyword: String,
}

impl SearchTestHarness {
    /// 创建新的测试工具实例
    pub fn new(engine_name: &str, timeout_secs: u64, result_limit: u32) -> Self {
        Self {
            engine_name: engine_name.to_string(),
            timeout_secs,
            result_limit,
            test_keyword: "gemini-3-pro".to_string(),
        }
    }

    /// 设置测试关键词
    pub fn with_keyword(mut self, keyword: &str) -> Self {
        self.test_keyword = keyword.to_string();
        self
    }

    /// 初始化日志系统（使用最小化 stderr logger）
    pub fn init_logging(&self) {
        let logger = StderrLogger;
        log::set_boxed_logger(Box::new(logger))
            .ok();
        log::set_max_level(log::LevelFilter::Info);
    }

    /// 运行测试引擎
    pub async fn run_engine_test<E: crate::search::engine_trait::SearchEngine>(
        &self,
        engine: E,
    ) -> Result<TestResult> {
        run_engine_test_with_output(
            &self.engine_name,
            engine,
            Some(&self.test_keyword),
            self.timeout_secs,
            Some(self.result_limit),
        )
        .await
    }

    /// 运行带超时的测试
    pub async fn run_engine_test_with_timeout<E: crate::search::engine_trait::SearchEngine>(
        &self,
        engine: E,
    ) -> Result<TestResult> {
        let timeout_duration = Duration::from_secs(self.timeout_secs);

        match timeout(timeout_duration, self.run_engine_test(engine)).await {
            Ok(result) => result,
            Err(_) => {
                info!(
                    "[TIMEOUT] {} 搜索超时 ({} 秒)",
                    self.engine_name, self.timeout_secs
                );
                Err(anyhow::anyhow!("Search timed out"))
            }
        }
    }

    /// 打印测试头信息
    pub fn print_header(&self) {
        info!("==========================================");
        info!("测试 {} 搜索引擎真实搜索功能", self.engine_name);
        info!("测试关键词: {}", self.test_keyword);
        info!("超时时间: {} 秒", self.timeout_secs);
        info!("==========================================");
    }

    /// 打印测试结果
    pub fn print_result(&self, result: &Result<TestResult>) {
        match result {
            Ok(test_result) => {
                info!("");
                info!("[SUCCESS] {} 搜索成功完成", self.engine_name);
                info!("  总结果数: {}", test_result.total);
                info!("  ✅ 可访问: {}", test_result.accessible);
                info!("  ❌ 不可访问: {}", test_result.inaccessible);
            }
            Err(e) => {
                info!("[FAILED] {} 搜索出错: {:?}", self.engine_name, e);
            }
        }
    }

    /// 打印测试完成信息
    pub fn print_footer(&self) {
        info!("==========================================");
        info!("测试完成");
    }

    /// 运行完整测试流程
    pub async fn run_full_test<E: crate::search::engine_trait::SearchEngine>(&self, engine: E) {
        self.print_header();

        let result = self.run_engine_test_with_timeout(engine).await;
        self.print_result(&result);

        self.print_footer();
    }
}

/// 搜索测试结果汇总工具
pub struct TestResultSummary {
    results: Vec<(String, Result<TestResult>)>,
}

impl TestResultSummary {
    /// 创建新的汇总实例
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
        }
    }

    /// 添加测试结果
    pub fn add_result(&mut self, name: &str, result: Result<TestResult>) {
        self.results.push((name.to_string(), result));
    }

    /// 打印汇总信息
    pub fn print_summary(&self) {
        info!("");
        info!("==========================================");
        info!("测试结果汇总");
        info!("==========================================");

        let mut total_accessible = 0;
        let mut total_inaccessible = 0;
        let mut total_success = 0;

        for (name, result) in &self.results {
            match result {
                Ok(test_result) => {
                    info!(
                        "  {}: 成功 {} 个, ✅ {} 个, ❌ {} 个",
                        name, test_result.total, test_result.accessible, test_result.inaccessible
                    );
                    total_accessible += test_result.accessible;
                    total_inaccessible += test_result.inaccessible;
                    total_success += 1;
                }
                Err(_) => {
                    info!("  {}: 测试失败", name);
                }
            }
        }

        info!("");
        info!("总计: 成功测试 {} 个引擎", total_success);
        info!("  ✅ 可访问: {} 个", total_accessible);
        info!("  ❌ 不可访问: {} 个", total_inaccessible);
    }
}

impl Default for TestResultSummary {
    fn default() -> Self {
        Self::new()
    }
}
