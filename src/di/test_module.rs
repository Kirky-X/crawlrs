// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Test module for Shaku dependency injection.
//!
//! This module provides test components and modules for unit and integration testing.
//! All test components use mock implementations that don't require external services.

use shaku::Component;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::di::engine_module::EngineModule;
use crate::di::infrastructure_module::InfrastructureModule;
use crate::di::search_module::SearchModule;
use crate::di::service_module::ServiceModule;
use crate::domain::models::task::{TaskStatus, TaskType};
use crate::domain::repositories::credits_repository::CreditsRepositoryError;
use crate::domain::repositories::credits_repository::{
    Credits, CreditsRepository, CreditsTransaction, CreditsTransactionType,
};
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepositoryError;
use crate::domain::repositories::storage_repository::StorageError;
use crate::domain::repositories::storage_repository::StorageRepository;
use crate::domain::repositories::task_repository::RepositoryError as TaskRepositoryError;
use crate::domain::repositories::task_repository::{Task, TaskRepository};
use crate::domain::services::rate_limiting_service::{RateLimitResult, RateLimitingService};
use crate::domain::services::team_service::TeamService;
use crate::domain::services::webhook_service::WebhookService;
use crate::presentation::middleware::team_semaphore::TeamSemaphore;
use crate::queue::task_queue::{QueueError, Task as QueueTask, TaskQueue};
use crate::utils::robots::RobotsCheckerTrait;
use anyhow::Result;
use std::time::Duration;

/// In-memory task repository for testing
#[derive(Component)]
#[shaku(interface = TaskRepository)]
pub struct InMemoryTaskRepository {
    /// Shared task storage
    tasks: Arc<Mutex<Vec<Task>>>,
    /// Auto-incrementing ID
    next_id: Arc<AtomicU64>,
}

impl InMemoryTaskRepository {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(Vec::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }
}

impl TaskRepository for InMemoryTaskRepository {
    async fn create(&self, task: &Task) -> Result<Task, TaskRepositoryError> {
        let mut tasks = self.tasks.lock().await;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut new_task = task.clone();
        new_task.id = Uuid::from_u128(id as u128);
        tasks.push(new_task.clone());
        Ok(new_task)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, TaskRepositoryError> {
        let tasks = self.tasks.lock().await;
        Ok(tasks.iter().find(|t| t.id == id).cloned())
    }

    async fn update(&self, task: &Task) -> Result<Task, TaskRepositoryError> {
        let mut tasks = self.tasks.lock().await;
        if let Some(pos) = tasks.iter().position(|t| t.id == task.id) {
            tasks[pos] = task.clone();
            Ok(task.clone())
        } else {
            Err(TaskRepositoryError::NotFound)
        }
    }

    async fn find_next_pending(&self) -> Result<Option<Task>, TaskRepositoryError> {
        let tasks = self.tasks.lock().await;
        Ok(tasks
            .iter()
            .find(|t| t.status == TaskStatus::Pending)
            .cloned())
    }

    async fn find_by_team_and_status(
        &self,
        _team_id: Uuid,
        _statuses: Option<Vec<TaskStatus>>,
    ) -> Result<Vec<Task>, TaskRepositoryError> {
        let tasks = self.tasks.lock().await;
        Ok(tasks.clone())
    }
}

impl Default for InMemoryTaskRepository {
    fn default() -> Self {
        Self::new()
    }
}

/// In-memory credits repository for testing
#[derive(Component)]
#[shaku(interface = CreditsRepository)]
pub struct InMemoryCreditsRepository {
    /// Shared credits storage
    credits: Arc<Mutex<HashMap<Uuid, i64>>>,
    /// Transaction history
    transactions: Arc<Mutex<Vec<CreditsTransaction>>>,
}

impl InMemoryCreditsRepository {
    pub fn new() -> Self {
        Self {
            credits: Arc::new(Mutex::new(HashMap::new())),
            transactions: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl CreditsRepository for InMemoryCreditsRepository {
    async fn get_balance(&self, team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
        let credits = self.credits.lock().await;
        Ok(*credits.get(&team_id).unwrap_or(&0))
    }

    async fn deduct_credits(
        &self,
        team_id: Uuid,
        amount: i64,
        transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<i64, CreditsRepositoryError> {
        let mut credits = self.credits.lock().await;
        let current = *credits.get(&team_id).unwrap_or(&0);
        let new_balance = current - amount;

        if new_balance < 0 {
            return Err(CreditsRepositoryError::InsufficientCredits {
                available: current,
                required: amount,
            });
        }

        credits.insert(team_id, new_balance);

        // Record transaction
        let mut transactions = self.transactions.lock().await;
        transactions.push(CreditsTransaction {
            id: Uuid::new_v4(),
            team_id,
            amount: -amount,
            transaction_type,
            description: None,
            reference_id: None,
            created_at: chrono::Utc::now(),
        });

        Ok(new_balance)
    }

    async fn add_credits(
        &self,
        team_id: Uuid,
        amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
    ) -> Result<i64, CreditsRepositoryError> {
        let mut credits = self.credits.lock().await;
        let current = *credits.get(&team_id).unwrap_or(&0);
        let new_balance = current + amount;
        credits.insert(team_id, new_balance);
        Ok(new_balance)
    }
}

impl Default for InMemoryCreditsRepository {
    fn default() -> Self {
        Self::new()
    }
}

/// Use std::collections::HashMap
use std::collections::HashMap;

/// Mock rate limiting service for testing
#[derive(Component)]
#[shaku(interface = RateLimitingService)]
pub struct MockRateLimitingService;

impl RateLimitingService for MockRateLimitingService {
    async fn check_rate_limit(
        &self,
        _api_key: &str,
        _endpoint: &str,
    ) -> Result<RateLimitResult, ()> {
        Ok(RateLimitResult {
            allowed: true,
            remaining: 100,
            reset_at: chrono::Utc::now(),
        })
    }

    async fn check_concurrency_limit(&self, _team_id: Uuid) -> Result<bool, ()> {
        Ok(true)
    }

    async fn acquire_permit(&self, _team_id: Uuid) -> Result<(), ()> {
        Ok(())
    }
}

/// Mock webhook service for testing
#[derive(Component)]
#[shaku(interface = WebhookService)]
pub struct MockWebhookService;

impl WebhookService for MockWebhookService {
    async fn send_webhook(
        &self,
        _url: &str,
        _event: &crate::domain::models::webhook::WebhookEvent,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

/// Mock team service for testing
#[derive(Component)]
#[shaku(interface = TeamService)]
pub struct MockTeamService;

impl TeamService for MockTeamService {
    async fn validate_geographic_restriction(
        &self,
        _team_id: Uuid,
        _ip_address: &str,
        _restrictions: &crate::domain::services::team_service::TeamGeoRestrictions,
    ) -> Result<crate::domain::services::team_service::GeoRestrictionResult, anyhow::Error> {
        Ok(crate::domain::services::team_service::GeoRestrictionResult::Allowed)
    }
}

/// Mock storage repository for testing
#[derive(Component)]
#[shaku(interface = StorageRepository)]
pub struct MockStorageRepository {
    /// In-memory storage
    data: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MockStorageRepository {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl StorageRepository for MockStorageRepository {
    async fn save(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        let mut storage = self.data.lock().await;
        storage.insert(key.to_string(), data.to_vec());
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let storage = self.data.lock().await;
        Ok(storage.get(key).cloned())
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let mut storage = self.data.lock().await;
        storage.remove(key);
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let storage = self.data.lock().await;
        Ok(storage.contains_key(key))
    }
}

impl Default for MockStorageRepository {
    fn default() -> Self {
        Self::new()
    }
}

/// Mock task queue for testing
#[derive(Component)]
#[shaku(interface = TaskQueue)]
pub struct MockTaskQueue {
    /// In-memory queue
    queue: Arc<Mutex<Vec<Task>>>,
}

impl MockTaskQueue {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl TaskQueue for MockTaskQueue {
    async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
        let mut queue = self.queue.lock().await;
        queue.push(task.clone());
        Ok(task)
    }

    async fn dequeue(&self, _worker_id: Uuid) -> Result<Option<Task>, QueueError> {
        let mut queue = self.queue.lock().await;
        Ok(queue.pop())
    }

    async fn complete(&self, _task_id: Uuid) -> Result<(), QueueError> {
        Ok(())
    }

    async fn fail(&self, _task_id: Uuid) -> Result<(), QueueError> {
        Ok(())
    }

    async fn cancel(&self, _task_id: Uuid) -> Result<(), QueueError> {
        Ok(())
    }
}

impl Default for MockTaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Mock robots checker for testing
#[derive(Component)]
#[shaku(interface = RobotsCheckerTrait)]
pub struct MockRobotsChecker;

#[async_trait]
impl RobotsCheckerTrait for MockRobotsChecker {
    async fn is_allowed(&self, _url_str: &str, _user_agent: &str) -> Result<bool> {
        Ok(true)
    }

    async fn get_crawl_delay(&self, _url_str: &str, _user_agent: &str) -> Result<Option<Duration>> {
        Ok(None)
    }
}

/// Mock team semaphore for testing
#[derive(Component)]
#[shaku(interface = ())]
pub struct MockTeamSemaphore {
    /// The actual team semaphore
    semaphore: Arc<TeamSemaphore>,
}

impl MockTeamSemaphore {
    pub fn new() -> Self {
        Self {
            semaphore: Arc::new(TeamSemaphore::new(100)),
        }
    }
}

impl Default for MockTeamSemaphore {
    fn default() -> Self {
        Self::new()
    }
}

/// Test module combining all mock components
///
/// This module is designed for unit and integration tests that don't require
/// external services like databases or Redis.
shaku::module! {
    pub TestModule {
        components = [
            InMemoryTaskRepository,
            InMemoryCreditsRepository,
            MockRateLimitingService,
            MockWebhookService,
            MockTeamService,
            MockStorageRepository,
            MockTaskQueue,
            MockRobotsChecker,
            MockTeamSemaphore,
        ],
        providers = []
    }
}

/// Helper function to create a TestModule
pub fn create_test_module() -> TestModule {
    TestModule::builder().build()
}
