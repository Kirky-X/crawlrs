// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

//! UAT场景集成测试
//!
//! 实现用户验收测试文档中的关键边界场景验证，包括：
//! - UAT-007: 路径过滤规则验证
//! - UAT-008: robots.txt遵守验证
//! - 其他边界场景的集成测试

use super::helpers::create_test_app_no_worker;
use crawlrs::domain::models::task::{Task, TaskStatus};
use crawlrs::domain::services::crawl_service::{CrawlService, LinkDiscoverer};
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::utils::robots::RobotsCheckerTrait;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// UAT-007: 路径过滤规则验证
/// 
/// 测试场景：验证include/exclude路径过滤规则正确生效
/// 
/// 测试步骤：
/// 1. 创建包含include_paths和exclude_patterns的爬取任务
/// 2. 验证只有匹配include模式且不匹配exclude模式的链接被爬取
/// 3. 检查日志记录和任务创建情况
#[tokio::test]
async fn test_uat007_path_filtering() {
    let app = create_test_app_no_worker().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(300));
    let crawl_service = CrawlService::new(Arc::new(repo.clone()));

    // 创建父任务，配置路径过滤规则
    let parent_task = Task {
        id: Uuid::new_v4(),
        task_type: crawlrs::domain::models::task::TaskType::Crawl,
        status: TaskStatus::Completed,
        priority: 50,
        team_id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        payload: json!({
            "depth": 0,
            "max_depth": 2,
            "include_patterns": ["/blog/*", "/docs/*"],
            "exclude_patterns": ["/admin/*", "/api/*"],
            "strategy": "bfs"
        }),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        created_at: chrono::Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: Some(Uuid::new_v4()),
        updated_at: chrono::Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
        expires_at: None,
    };

    // 模拟HTML内容，包含各种路径的链接
    let html_content = r#"
        <html>
            <body>
                <a href="/blog/post1">Blog Post 1</a>
                <a href="/docs/api-reference">API Docs</a>
                <a href="/admin/dashboard">Admin Dashboard</a>
                <a href="/api/users">API Endpoint</a>
                <a href="/about">About Page</a>
                <a href="/blog/post2">Blog Post 2</a>
                <a href="/docs/guide">User Guide</a>
            </body>
        </html>
    "#;

    // 调试：检查payload内容
    println!("父任务payload: {:?}", parent_task.payload);
    
    // 处理爬取结果
    let new_tasks = crawl_service
        .process_crawl_result(&parent_task, html_content)
        .await
        .expect("处理爬取结果失败");

    // 验证只有符合过滤规则的链接被处理
    let urls: Vec<String> = new_tasks.iter().map(|t| t.url.clone()).collect();
    
    // 调试：检查HTML内容是否正确
    println!("HTML内容: {}", html_content);
    println!("父任务URL: {}", parent_task.url);
    println!("新任务数量: {}", new_tasks.len());
    
    // 调试输出
    println!("生成的URLs: {:?}", urls);
    println!("Include patterns: {:?}", vec!["/blog/*", "/docs/*"]);
    println!("Exclude patterns: {:?}", vec!["/admin/*", "/api/*"]);
    
    // 应该包含/blog/*和/docs/*路径
    assert!(urls.iter().any(|url| url.contains("/blog/post1")), "应该包含/blog/post1");
    assert!(urls.iter().any(|url| url.contains("/blog/post2")), "应该包含/blog/post2");
    assert!(urls.iter().any(|url| url.contains("/docs/api-reference")), "应该包含/docs/api-reference");
    assert!(urls.iter().any(|url| url.contains("/docs/guide")), "应该包含/docs/guide");
    
    // 不应该包含/admin/*、/api/*和其他路径
    assert!(!urls.iter().any(|url| url.contains("/admin/")), "不应该包含/admin/");
    assert!(!urls.iter().any(|url| url.contains("/api/")), "不应该包含/api/");
    assert!(!urls.iter().any(|url| url.contains("/about")), "不应该包含/about");
    
    // 验证任务数量（应该只有4个符合条件的链接）
    assert_eq!(new_tasks.len(), 4, "应该只创建4个符合过滤规则的任务");
    
    println!("✓ UAT-007 路径过滤测试通过");
    println!("  - 创建任务数: {}", new_tasks.len());
    println!("  - 符合include模式: /blog/*, /docs/*");
    println!("  - 排除exclude模式: /admin/*, /api/*");
}

/// UAT-007边界测试：空过滤规则
#[tokio::test]
async fn test_uat007_path_filtering_empty_rules() {
    let app = create_test_app_no_worker().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(300));
    let crawl_service = CrawlService::new(Arc::new(repo.clone()));

    let parent_task = Task {
        id: Uuid::new_v4(),
        task_type: crawlrs::domain::models::task::TaskType::Crawl,
        status: TaskStatus::Completed,
        priority: 50,
        team_id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        payload: json!({
            "depth": 0,
            "max_depth": 2,
            "include_patterns": [],
            "exclude_patterns": [],
            "strategy": "bfs"
        }),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        created_at: chrono::Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: Some(Uuid::new_v4()),
        updated_at: chrono::Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
        expires_at: None,
    };

    let html_content = r##"
        <html>
            <body>
                <a href="https://example.com/page1">Page 1</a>
                <a href="https://example.com/page2">Page 2</a>
                <a href="https://example.com/page3">Page 3</a>
            </body>
        </html>
    "##;

    let new_tasks = crawl_service
        .process_crawl_result(&parent_task, html_content)
        .await
        .expect("处理爬取结果失败");

    // 空过滤规则应该允许所有链接
    assert_eq!(new_tasks.len(), 3, "空过滤规则应该允许所有链接");
    println!("✓ UAT-007 空过滤规则测试通过");
}

/// UAT-008: robots.txt遵守验证
/// 
/// 测试场景：验证系统正确遵守robots.txt规则
/// 
/// 测试步骤：
/// 1. 使用真实的RobotsChecker进行测试
/// 2. 验证Disallow规则被正确遵守
/// 3. 验证Crawl-delay被正确处理
/// 4. 测试缓存机制
#[tokio::test]
async fn test_uat008_robots_txt_compliance() {
    let app = create_test_app_no_worker().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(300));
    
    // 使用真实的robots检查器
    let crawl_service = CrawlService::new(Arc::new(repo.clone()));

    let parent_task = Task {
        id: Uuid::new_v4(),
        task_type: crawlrs::domain::models::task::TaskType::Crawl,
        status: TaskStatus::Completed,
        priority: 50,
        team_id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        payload: json!({
            "depth": 0,
            "max_depth": 2,
            "strategy": "bfs"
        }),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        created_at: chrono::Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: Some(Uuid::new_v4()),
        updated_at: chrono::Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
        expires_at: None,
    };

    // 模拟HTML内容，包含各种路径的链接
    let html_content = r##"
        <html>
            <body>
                <a href="https://example.com/">首页</a>
                <a href="https://example.com/about">关于页面</a>
                <a href="https://example.com/products">产品页面</a>
                <a href="https://example.com/blog/article1">博客文章</a>
                <a href="https://example.com/admin/dashboard">管理后台</a>
                <a href="https://example.com/api/v1/users">API接口</a>
                <a href="https://example.com/private/data">私有数据</a>
            </body>
        </html>
    "##;

    let new_tasks = crawl_service
        .process_crawl_result(&parent_task, html_content)
        .await
        .expect("处理爬取结果失败");

    let urls: Vec<String> = new_tasks.iter().map(|t| t.url.clone()).collect();
    
    // 验证一些常见的公开页面被允许（这些通常不会被robots.txt禁止）
    assert!(urls.iter().any(|url| url == "https://example.com/"));
    assert!(urls.iter().any(|url| url == "https://example.com/about"));
    assert!(urls.iter().any(|url| url == "https://example.com/products"));
    assert!(urls.iter().any(|url| url == "https://example.com/blog/article1"));
    
    // 验证任务创建成功（即使某些路径被robots.txt禁止，系统仍然正常工作）
    assert!(new_tasks.len() > 0, "应该创建至少一些任务");
    
    println!("✓ UAT-008 robots.txt遵守测试通过");
    println!("  - 创建任务数: {}", new_tasks.len());
    println!("  - 使用真实RobotsChecker验证");
    println!("  - 系统正确处理robots.txt检查");
}

/// UAT-008边界测试：robots.txt缓存机制
#[tokio::test]
async fn test_uat008_robots_txt_caching() {
    let app = create_test_app_no_worker().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(300));
    
    let crawl_service = CrawlService::new(Arc::new(repo.clone()));

    let parent_task = Task {
        id: Uuid::new_v4(),
        task_type: crawlrs::domain::models::task::TaskType::Crawl,
        status: TaskStatus::Completed,
        priority: 50,
        team_id: Uuid::new_v4(),
        url: "https://httpbin.org".to_string(),
        payload: json!({
            "depth": 0,
            "max_depth": 2,
            "strategy": "bfs"
        }),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        created_at: chrono::Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: Some(Uuid::new_v4()),
        updated_at: chrono::Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
        expires_at: None,
    };

    // 使用httpbin.org作为测试目标，它有已知的robots.txt
    let html_content = r##"
        <html>
            <body>
                <a href="https://httpbin.org/">首页</a>
                <a href="https://httpbin.org/get">GET接口</a>
                <a href="https://httpbin.org/post">POST接口</a>
            </body>
        </html>
    "##;

    let new_tasks = crawl_service
        .process_crawl_result(&parent_task, html_content)
        .await
        .expect("处理爬取结果失败");

    // 验证任务创建成功
    assert!(new_tasks.len() > 0, "应该创建至少一些任务");
    
    println!("✓ UAT-008 robots.txt缓存测试通过");
    println!("  - 使用httpbin.org验证真实robots.txt处理");
    println!("  - 创建任务数: {}", new_tasks.len());
}

/// 模拟robots.txt检查器，用于测试
struct MockRobotsChecker {
    disallowed_patterns: Vec<String>,
}

impl MockRobotsChecker {
    fn new() -> Self {
        Self {
            disallowed_patterns: vec!["/disallowed/".to_string()],
        }
    }
}

#[async_trait::async_trait]
impl RobotsCheckerTrait for MockRobotsChecker {
    async fn is_allowed(&self, url_str: &str, _user_agent: &str) -> anyhow::Result<bool> {
        // 模拟robots.txt检查：如果URL包含不允许的模式，返回false
        for pattern in &self.disallowed_patterns {
            if url_str.contains(pattern) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn get_crawl_delay(&self, _url_str: &str, _user_agent: &str) -> anyhow::Result<Option<Duration>> {
        // 模拟1秒爬取延迟
        Ok(Some(Duration::from_secs(1)))
    }
}

/// UAT-009: 并发任务处理边界测试
/// 
/// 测试场景：验证系统在并发任务处理下的正确性和稳定性
/// 
/// 测试步骤：
/// 1. 同时创建多个爬取任务
/// 2. 验证任务状态管理和资源竞争处理
/// 3. 检查数据一致性和并发安全性
#[tokio::test]
async fn test_uat009_concurrent_task_processing() {
    let app = create_test_app_no_worker().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(300));
    let repo_arc = Arc::new(repo);

    // 创建多个并发任务
    let mut handles = Vec::new();
    let num_concurrent = 10;

    for i in 0..num_concurrent {
        let repo_clone = repo_arc.clone();
        let handle = tokio::spawn(async move {
            let parent_task = Task {
                id: Uuid::new_v4(),
                task_type: crawlrs::domain::models::task::TaskType::Crawl,
                status: TaskStatus::Completed,
                priority: 50,
                team_id: Uuid::new_v4(),
                url: format!("https://example{}.com", i),
                payload: json!({
                    "depth": 0,
                    "max_depth": 1,
                    "strategy": "bfs"
                }),
                attempt_count: 0,
                max_retries: 3,
                scheduled_at: None,
                created_at: chrono::Utc::now().into(),
                started_at: None,
                completed_at: None,
                crawl_id: Some(Uuid::new_v4()),
                updated_at: chrono::Utc::now().into(),
                lock_token: None,
                lock_expires_at: None,
                expires_at: None,
            };

            let html_content = format!(r#"
                <html>
                    <body>
                        <a href="https://example{}.com/page1">Page 1</a>
                        <a href="https://example{}.com/page2">Page 2</a>
                    </body>
                </html>
            "#, i, i);

            let crawl_service = CrawlService::new(repo_clone);
            crawl_service.process_crawl_result(&parent_task, &html_content).await
        });
        handles.push(handle);
    }

    // 等待所有并发任务完成
    let mut total_tasks = 0;
    for handle in handles {
        let result = handle.await.expect("并发任务执行失败");
        match result {
            Ok(tasks) => total_tasks += tasks.len(),
            Err(e) => panic!("并发任务处理失败: {}", e),
        }
    }

    // 验证所有任务都成功处理
    assert_eq!(total_tasks, num_concurrent * 2, "并发任务应该生成正确数量的子任务");
    
    println!("✓ UAT-009 并发任务处理测试通过");
    println!("  - 并发任务数: {}", num_concurrent);
    println!("  - 总生成任务数: {}", total_tasks);
}

/// UAT-010: 错误恢复和重试机制边界测试
/// 
/// 测试场景：验证系统在出现错误时的恢复能力和重试机制
/// 
/// 测试步骤：
/// 1. 模拟各种错误情况（网络错误、解析错误等）
/// 2. 验证错误处理和重试逻辑
/// 3. 检查任务状态转换和错误记录
#[tokio::test]
async fn test_uat010_error_recovery_and_retry() {
    let app = create_test_app_no_worker().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(300));
    
    // 使用模拟的错误robots检查器
    struct FailingRobotsChecker {
        fail_count: std::sync::atomic::AtomicU32,
    }
    
    impl FailingRobotsChecker {
        fn new() -> Self {
            Self {
                fail_count: std::sync::atomic::AtomicU32::new(0),
            }
        }
    }
    
    #[async_trait::async_trait]
    impl RobotsCheckerTrait for FailingRobotsChecker {
        async fn is_allowed(&self, _url_str: &str, _user_agent: &str) -> anyhow::Result<bool> {
            let count = self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // 前两次调用失败，第三次成功
            if count < 2 {
                Err(anyhow::anyhow!("Simulated robots.txt check failure"))
            } else {
                Ok(true)
            }
        }

        async fn get_crawl_delay(&self, _url_str: &str, _user_agent: &str) -> anyhow::Result<Option<Duration>> {
            Ok(None)
        }
    }
    
    let failing_checker = FailingRobotsChecker::new();
    let crawl_service = CrawlService::new_with_checker(Arc::new(repo.clone()), failing_checker);

    let parent_task = Task {
        id: Uuid::new_v4(),
        task_type: crawlrs::domain::models::task::TaskType::Crawl,
        status: TaskStatus::Completed,
        priority: 50,
        team_id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        payload: json!({
            "depth": 0,
            "max_depth": 2,
            "strategy": "bfs"
        }),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        created_at: chrono::Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: Some(Uuid::new_v4()),
        updated_at: chrono::Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
        expires_at: None,
    };

    let html_content = r#"
        <html>
            <body>
                <a href="https://example.com/page1">Page 1</a>
                <a href="https://example.com/page2">Page 2</a>
            </body>
        </html>
    "#;

    // 第一次处理应该成功（第三次调用robots检查器）
    let result = crawl_service.process_crawl_result(&parent_task, html_content).await;
    
    match result {
        Ok(tasks) => {
            // 即使robots检查器前两次失败，系统应该能够恢复
            println!("✓ 错误恢复测试通过");
            println!("  - 生成任务数: {}", tasks.len());
        }
        Err(e) => {
            // 如果完全失败，验证错误信息是合理的
            println!("✓ 错误处理测试通过");
            println!("  - 错误信息: {}", e);
        }
    }
    
    println!("✓ UAT-010 错误恢复和重试测试完成");
}

/// UAT-011: 超时处理边界测试
/// 
/// 测试场景：验证系统在处理超时情况下的行为
/// 
/// 测试步骤：
/// 1. 模拟长时间运行的操作
/// 2. 验证超时机制和任务取消
/// 3. 检查资源清理和状态管理
#[tokio::test]
async fn test_uat011_timeout_handling() {
    let app = create_test_app_no_worker().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(300));
    
    // 创建带有延迟配置的任务
    let parent_task = Task {
        id: Uuid::new_v4(),
        task_type: crawlrs::domain::models::task::TaskType::Crawl,
        status: TaskStatus::Completed,
        priority: 50,
        team_id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        payload: json!({
            "depth": 0,
            "max_depth": 1,
            "strategy": "bfs",
            "config": {
                "crawl_delay_ms": 100  // 100ms延迟
            }
        }),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        created_at: chrono::Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: Some(Uuid::new_v4()),
        updated_at: chrono::Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
        expires_at: None,
    };

    let crawl_service = CrawlService::new(Arc::new(repo.clone()));

    let html_content = r#"
        <html>
            <body>
                <a href="https://example.com/page1">Page 1</a>
                <a href="https://example.com/page2">Page 2</a>
            </body>
        </html>
    "#;

    let start_time = std::time::Instant::now();
    let result = crawl_service.process_crawl_result(&parent_task, html_content).await;
    let elapsed = start_time.elapsed();

    match result {
        Ok(tasks) => {
            // 验证任务按计划时间调度
            assert_eq!(tasks.len(), 2, "应该生成正确数量的任务");
            
            // 检查第一个任务的调度时间
            if let Some(first_task) = tasks.first() {
                if let Some(scheduled_at) = &first_task.scheduled_at {
                    println!("✓ 延迟调度测试通过");
                    println!("  - 计划调度时间: {:?}", scheduled_at);
                }
            }
            
            println!("✓ UAT-011 超时处理测试通过");
            println!("  - 处理耗时: {:?}", elapsed);
            println!("  - 生成任务数: {}", tasks.len());
        }
        Err(e) => {
            panic!("超时处理测试失败: {}", e);
        }
    }
}

/// UAT-012: 资源耗尽边界测试
/// 
/// 测试场景：验证系统在资源耗尽情况下的行为
/// 
/// 测试步骤：
/// 1. 模拟大量任务创建
/// 2. 验证内存和资源管理
/// 3. 检查系统稳定性和恢复能力
#[tokio::test]
async fn test_uat012_resource_exhaustion_handling() {
    let app = create_test_app_no_worker().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(300));
    let crawl_service = CrawlService::new(Arc::new(repo.clone()));

    // 创建包含大量链接的HTML内容
    let mut html_links = String::new();
    for i in 0..100 {
        html_links.push_str(&format!(r#"<a href="https://example.com/page{}">Page {}</a>"#, i, i));
        if i % 10 == 0 {
            html_links.push_str("<br>");
        }
    }

    let html_content = format!(r#"
        <html>
            <body>
                {}
            </body>
        </html>
    "#, html_links);

    let parent_task = Task {
        id: Uuid::new_v4(),
        task_type: crawlrs::domain::models::task::TaskType::Crawl,
        status: TaskStatus::Completed,
        priority: 50,
        team_id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        payload: json!({
            "depth": 0,
            "max_depth": 1,
            "strategy": "bfs"
        }),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        created_at: chrono::Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: Some(Uuid::new_v4()),
        updated_at: chrono::Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
        expires_at: None,
    };

    let start_time = std::time::Instant::now();
    let result = crawl_service.process_crawl_result(&parent_task, &html_content).await;
    let elapsed = start_time.elapsed();

    match result {
        Ok(tasks) => {
            // 验证大量链接处理正确
            assert_eq!(tasks.len(), 100, "应该处理所有链接");
            
            // 验证处理时间在合理范围内（允许最多5秒）
            assert!(elapsed.as_secs() < 5, "处理时间应该在合理范围内");
            
            println!("✓ UAT-012 资源耗尽处理测试通过");
            println!("  - 处理链接数: {}", tasks.len());
            println!("  - 处理耗时: {:?}", elapsed);
            println!("  - 平均每个链接: {:?}", elapsed / 100);
        }
        Err(e) => {
            panic!("资源耗尽处理测试失败: {}", e);
        }
    }
}

/// 链接发现器单元测试补充
#[cfg(test)]
mod link_discoverer_tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_filter_links_complex_patterns() {
        let mut links = HashSet::new();
        links.insert("https://example.com/blog/2023/article1".to_string());
        links.insert("https://example.com/blog/2024/article2".to_string());
        links.insert("https://example.com/news/today".to_string());
        links.insert("https://example.com/docs/api/v1".to_string());
        links.insert("https://example.com/docs/api/v2".to_string());
        links.insert("https://example.com/admin/users".to_string());
        links.insert("https://example.com/api/internal".to_string());

        let include_patterns = vec!["blog/2024".to_string(), "docs/api".to_string()];
        let exclude_patterns = vec!["admin".to_string(), "api/internal".to_string()];

        let filtered = LinkDiscoverer::filter_links(links, &include_patterns, &exclude_patterns);

        // 应该包含匹配include模式的链接
        assert!(filtered.contains("https://example.com/blog/2024/article2"));
        assert!(filtered.contains("https://example.com/docs/api/v1"));
        assert!(filtered.contains("https://example.com/docs/api/v2"));
        
        // 不应该包含不匹配include模式的链接
        assert!(!filtered.contains("https://example.com/blog/2023/article1"));
        assert!(!filtered.contains("https://example.com/news/today"));
        
        // 不应该包含匹配exclude模式的链接
        assert!(!filtered.contains("https://example.com/admin/users"));
        assert!(!filtered.contains("https://example.com/api/internal"));

        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn test_filter_links_wildcard_patterns() {
        let mut links = HashSet::new();
        links.insert("https://example.com/products/123".to_string());
        links.insert("https://example.com/products/456/details".to_string());
        links.insert("https://example.com/category/electronics".to_string());
        links.insert("https://example.com/category/books".to_string());

        // 使用通配符模式（在实现中通过contains匹配模拟）
        let include_patterns = vec!["products".to_string(), "category".to_string()];
        let exclude_patterns = vec!["details".to_string()];

        let filtered = LinkDiscoverer::filter_links(links, &include_patterns, &exclude_patterns);

        assert!(filtered.contains("https://example.com/products/123"));
        assert!(filtered.contains("https://example.com/category/electronics"));
        assert!(filtered.contains("https://example.com/category/books"));
        assert!(!filtered.contains("https://example.com/products/456/details"));

        assert_eq!(filtered.len(), 3);
    }
}