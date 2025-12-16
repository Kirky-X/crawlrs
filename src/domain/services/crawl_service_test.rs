#[cfg(test)]
mod tests {
    use crate::domain::models::task::{Task, TaskStatus, TaskType};
    use crate::domain::repositories::task_repository::{RepositoryError, TaskRepository};
    use crate::domain::services::crawl_service::{CrawlService, LinkDiscoverer};
    use crate::utils::robots::RobotsCheckerTrait;
    use anyhow::Result;
    use async_trait::async_trait;
    use chrono::Utc;
    use mockall::mock;
    use mockall::predicate::*;
    use std::collections::HashSet;
    use std::sync::Arc;
    use uuid::Uuid;

    // --- Mocks ---

    mock! {
        pub TaskRepository {}
        #[async_trait]
        impl TaskRepository for TaskRepository {
            async fn create(&self, task: &Task) -> Result<Task, RepositoryError>;
            async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError>;
            async fn update(&self, task: &Task) -> Result<Task, RepositoryError>;
            async fn acquire_next(&self, worker_id: Uuid) -> Result<Option<Task>, RepositoryError>;
            async fn mark_completed(&self, id: Uuid) -> Result<(), RepositoryError>;
            async fn mark_failed(&self, id: Uuid) -> Result<(), RepositoryError>;
            async fn mark_cancelled(&self, id: Uuid) -> Result<(), RepositoryError>;
            async fn exists_by_url(&self, url: &str) -> Result<bool, RepositoryError>;
            async fn reset_stuck_tasks(&self, timeout: chrono::Duration) -> Result<u64, RepositoryError>;
            async fn cancel_tasks_by_crawl_id(&self, crawl_id: Uuid) -> Result<u64, RepositoryError>;
            async fn expire_tasks(&self) -> Result<u64, RepositoryError>;
            async fn find_by_crawl_id(&self, crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError>;
        }
    }

    mock! {
        pub RobotsChecker {}
        #[async_trait]
        impl RobotsCheckerTrait for RobotsChecker {
            async fn is_allowed(&self, url_str: &str, user_agent: &str) -> Result<bool>;
            async fn get_crawl_delay(&self, url_str: &str, user_agent: &str) -> Result<Option<std::time::Duration>>;
        }
    }

    // --- LinkDiscoverer Tests ---

    #[test]
    fn test_extract_links() {
        let html = r##"
            <html>
                <body>
                    <a href="https://example.com/page1">Page 1</a>
                    <a href="/page2">Page 2</a>
                    <a href="page3.html">Page 3</a>
                    <a href="#fragment">Fragment</a>
                    <a href="mailto:test@example.com">Email</a>
                    <a href="javascript:void(0)">JS</a>
                </body>
            </html>
        "##;
        let base_url = "https://example.com";

        let links = LinkDiscoverer::extract_links(html, base_url).unwrap();

        assert!(links.contains("https://example.com/page1"));
        assert!(links.contains("https://example.com/page2"));
        assert!(links.contains("https://example.com/page3.html"));
        assert_eq!(links.len(), 3);
    }

    #[test]
    fn test_filter_links() {
        let mut links = HashSet::new();
        links.insert("https://example.com/blog/1".to_string());
        links.insert("https://example.com/shop/item".to_string());
        links.insert("https://example.com/about".to_string());

        let include_patterns = vec!["blog".to_string(), "about".to_string()];
        let exclude_patterns = vec!["shop".to_string()];

        let filtered = LinkDiscoverer::filter_links(links, &include_patterns, &exclude_patterns);

        assert!(filtered.contains("https://example.com/blog/1"));
        assert!(filtered.contains("https://example.com/about"));
        assert!(!filtered.contains("https://example.com/shop/item"));
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_links_no_include() {
        let mut links = HashSet::new();
        links.insert("https://example.com/blog/1".to_string());
        links.insert("https://example.com/shop/item".to_string());

        let include_patterns = vec![];
        let exclude_patterns = vec!["shop".to_string()];

        let filtered = LinkDiscoverer::filter_links(links, &include_patterns, &exclude_patterns);

        assert!(filtered.contains("https://example.com/blog/1"));
        assert!(!filtered.contains("https://example.com/shop/item"));
        assert_eq!(filtered.len(), 1);
    }

    // --- CrawlService Tests ---

    fn create_dummy_task() -> Task {
        Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Scrape,
            status: TaskStatus::Active,
            priority: 0,
            team_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            payload: serde_json::json!({
                "depth": 0,
                "max_depth": 3,
                "include_patterns": [],
                "exclude_patterns": [],
                "strategy": "bfs"
            }),
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: Utc::now().into(),
            started_at: Some(Utc::now().into()),
            completed_at: None,
            crawl_id: Some(Uuid::new_v4()),
            updated_at: Utc::now().into(),
            lock_token: Some(Uuid::new_v4()),
            lock_expires_at: Some(Utc::now().into()),
        }
    }

    #[tokio::test]
    async fn test_process_crawl_result_creates_tasks() {
        let mut mock_repo = MockTaskRepository::new();
        let mut mock_robots = MockRobotsChecker::new();

        // Expect exists_by_url to be called for found links and return false (not duplicate)
        mock_repo
            .expect_exists_by_url()
            .with(eq("https://example.com/page1"))
            .times(1)
            .returning(|_| Ok(false));

        // Expect create to be called once
        mock_repo
            .expect_create()
            .times(1)
            .returning(|t| Ok(t.clone()));

        // Expect robots check to pass
        mock_robots
            .expect_is_allowed()
            .with(eq("https://example.com/page1"), eq("Crawlrs/0.1.0"))
            .times(1)
            .returning(|_, _| Ok(true));

        let service = CrawlService::new_with_checker(Arc::new(mock_repo), mock_robots);
        let parent_task = create_dummy_task();
        let html = r#"<a href="https://example.com/page1">Link</a>"#;

        let result = service
            .process_crawl_result(&parent_task, html)
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].url, "https://example.com/page1");
        assert_eq!(result[0].payload["depth"], 1);
    }

    #[tokio::test]
    async fn test_process_crawl_result_respects_max_depth() {
        let mut mock_repo = MockTaskRepository::new();
        let mock_robots = MockRobotsChecker::new();

        // No interactions expected with repo or robots if max depth reached
        mock_repo.expect_create().times(0);

        let service = CrawlService::new_with_checker(Arc::new(mock_repo), mock_robots);
        let mut parent_task = create_dummy_task();
        // Set current depth to max_depth
        parent_task.payload["depth"] = serde_json::json!(3);
        parent_task.payload["max_depth"] = serde_json::json!(3);

        let html = r#"<a href="https://example.com/page1">Link</a>"#;

        let result = service
            .process_crawl_result(&parent_task, html)
            .await
            .unwrap();

        assert_eq!(result.len(), 0);
    }

    #[tokio::test]
    async fn test_process_crawl_result_deduplication() {
        let mut mock_repo = MockTaskRepository::new();
        let mut mock_robots = MockRobotsChecker::new();

        // Expect exists_by_url to return true (duplicate)
        mock_repo
            .expect_exists_by_url()
            .with(eq("https://example.com/page1"))
            .times(1)
            .returning(|_| Ok(true));

        // Expect create NOT to be called
        mock_repo.expect_create().times(0);

        // Expect robots check to pass
        mock_robots
            .expect_is_allowed()
            .times(1)
            .returning(|_, _| Ok(true));

        let service = CrawlService::new_with_checker(Arc::new(mock_repo), mock_robots);
        let parent_task = create_dummy_task();
        let html = r#"<a href="https://example.com/page1">Link</a>"#;

        let result = service
            .process_crawl_result(&parent_task, html)
            .await
            .unwrap();

        assert_eq!(result.len(), 0);
    }

    #[tokio::test]
    async fn test_process_crawl_result_robots_disallowed() {
        let mock_repo = MockTaskRepository::new();
        let mut mock_robots = MockRobotsChecker::new();

        // Expect robots check to fail
        mock_robots
            .expect_is_allowed()
            .with(eq("https://example.com/page1"), eq("Crawlrs/0.1.0"))
            .times(1)
            .returning(|_, _| Ok(false));

        let service = CrawlService::new_with_checker(Arc::new(mock_repo), mock_robots);
        let parent_task = create_dummy_task();
        let html = r#"<a href="https://example.com/page1">Link</a>"#;

        let result = service
            .process_crawl_result(&parent_task, html)
            .await
            .unwrap();

        assert_eq!(result.len(), 0);
    }

    #[tokio::test]
    async fn test_process_crawl_result_dfs_priority() {
        let mut mock_repo = MockTaskRepository::new();
        let mut mock_robots = MockRobotsChecker::new();

        mock_repo.expect_exists_by_url().returning(|_| Ok(false));
        mock_repo
            .expect_create()
            .times(1)
            .returning(|t| Ok(t.clone()));
        mock_robots.expect_is_allowed().returning(|_, _| Ok(true));

        let service = CrawlService::new_with_checker(Arc::new(mock_repo), mock_robots);
        let mut parent_task = create_dummy_task();
        parent_task.payload["strategy"] = serde_json::json!("dfs");
        parent_task.priority = 10;

        let html = r#"<a href="https://example.com/page1">Link</a>"#;

        let result = service
            .process_crawl_result(&parent_task, html)
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        // DFS should increase priority (LIFO-like behavior in priority queue)
        assert_eq!(result[0].priority, 20);
    }

    #[tokio::test]
    async fn test_process_crawl_result_with_delay() {
        let mut mock_repo = MockTaskRepository::new();
        let mut mock_robots = MockRobotsChecker::new();

        mock_repo.expect_exists_by_url().returning(|_| Ok(false));
        mock_repo
            .expect_create()
            .times(1)
            .returning(|t| Ok(t.clone()));
        mock_robots.expect_is_allowed().returning(|_, _| Ok(true));

        let service = CrawlService::new_with_checker(Arc::new(mock_repo), mock_robots);
        let mut parent_task = create_dummy_task();
        parent_task.payload["config"] = serde_json::json!({
            "crawl_delay_ms": 1000
        });

        let html = r#"<a href="https://example.com/page1">Link</a>"#;

        let result = service
            .process_crawl_result(&parent_task, html)
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].scheduled_at.is_some());

        // Verify scheduled_at is roughly 1 second in the future
        let scheduled_at = result[0].scheduled_at.unwrap();
        let now = Utc::now();
        let diff = scheduled_at.signed_duration_since(now).num_milliseconds();
        // Allow some tolerance
        assert!(diff >= 900 && diff <= 1100);
    }
}
