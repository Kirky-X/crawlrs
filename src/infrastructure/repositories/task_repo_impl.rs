// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::task::{Task, TaskStatus};
use crate::domain::repositories::task_repository::{
    RepositoryError, TaskQueryParams, TaskRepository,
};
use crate::infrastructure::database::entities::task as task_entity;
use async_trait::async_trait;
use chrono::{DateTime, Duration, FixedOffset, Utc};
use sea_orm::{
    sea_query::{Expr, LockBehavior, LockType},
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set, TransactionTrait,
};
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

/// 任务仓库实现
///
/// 基于SeaORM实现的任务数据访问层
#[derive(Clone)]
pub struct TaskRepositoryImpl {
    /// 数据库连接
    db: Arc<DatabaseConnection>,
    /// 锁持续时间
    lock_duration: Duration,
}

impl TaskRepositoryImpl {
    /// 创建新的任务仓库实例
    ///
    /// # 参数
    ///
    /// * `db` - 数据库连接
    ///
    /// # 返回值
    ///
    /// 返回新的任务仓库实例
    pub fn new(db: Arc<DatabaseConnection>, lock_duration: Duration) -> Self {
        Self { db, lock_duration }
    }
}

impl From<task_entity::Model> for Task {
    fn from(model: task_entity::Model) -> Self {
        Self {
            id: model.id,
            task_type: model.task_type.parse().unwrap_or_default(),
            status: model.status.parse().unwrap_or_default(),
            priority: model.priority,
            team_id: model.team_id,
            url: model.url,
            payload: model.payload,
            attempt_count: model.attempt_count,
            max_retries: model.max_retries,
            scheduled_at: model.scheduled_at,
            expires_at: model.expires_at,
            created_at: model.created_at,
            started_at: model.started_at,
            completed_at: model.completed_at,
            crawl_id: model.crawl_id,
            updated_at: model.updated_at,
            lock_token: model.lock_token,
            lock_expires_at: model.lock_expires_at,
        }
    }
}

impl From<Task> for task_entity::ActiveModel {
    fn from(task: Task) -> Self {
        Self {
            id: Set(task.id),
            task_type: Set(task.task_type.to_string()),
            status: Set(task.status.to_string()),
            priority: Set(task.priority),
            team_id: Set(task.team_id),
            url: Set(task.url.clone()),
            payload: Set(task.payload.clone()),
            attempt_count: Set(task.attempt_count),
            max_retries: Set(task.max_retries),
            scheduled_at: Set(task.scheduled_at),
            expires_at: Set(task.expires_at),
            created_at: Set(task.created_at),
            started_at: Set(task.started_at),
            completed_at: Set(task.completed_at),
            crawl_id: Set(task.crawl_id),
            updated_at: Set(task.updated_at),
            lock_token: Set(task.lock_token),
            lock_expires_at: Set(task.lock_expires_at),
        }
    }
}

#[async_trait]
impl TaskRepository for TaskRepositoryImpl {
    async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
        let model: task_entity::ActiveModel = task.clone().into();

        println!("DEBUG: Creating task {} with status {}", task.id, task.status);

        model.insert(self.db.as_ref()).await?;
        Ok(task.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError> {
        let model = task_entity::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await?;

        Ok(model.map(Into::into))
    }

    async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
        let mut model: task_entity::ActiveModel = task.clone().into();

        model.status = Set(task.status.to_string());
        model.attempt_count = Set(task.attempt_count);
        model.scheduled_at = Set(task.scheduled_at);
        model.started_at = Set(task.started_at);
        model.completed_at = Set(task.completed_at);

        let updated_model = model.update(self.db.as_ref()).await?;
        Ok(updated_model.into())
    }

    async fn acquire_next(&self, worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
        let txn = self.db.begin().await?;

        println!("DEBUG: acquire_next called by worker {}", worker_id);
        let now = Utc::now();
        println!("DEBUG: Current time: {}", now);

        let task = task_entity::Entity::find()
            .filter(
                Condition::any()
                    .add(task_entity::Column::Status.eq(TaskStatus::Queued.to_string()))
                    .add(
                        Condition::all()
                            .add(task_entity::Column::Status.eq(TaskStatus::Active.to_string()))
                            .add(task_entity::Column::LockExpiresAt.lt(now)),
                    ),
            )
            .filter(
                Condition::any()
                    .add(task_entity::Column::ScheduledAt.is_null())
                    .add(task_entity::Column::ScheduledAt.lte(now)),
            )
            .order_by_desc(task_entity::Column::Priority)
            .order_by_asc(task_entity::Column::CreatedAt)
            .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
            .one(&txn)
            .await?;

        println!("DEBUG: Found task: {:?}", task.as_ref().map(|t| t.id));
        if let Some(ref t) = task {
            println!(
                "DEBUG: Task status: {}, lock_expires_at: {:?}",
                t.status, t.lock_expires_at
            );
        }

        if let Some(task) = task {
            let mut active: task_entity::ActiveModel = task.into();
            active.lock_token = Set(Some(worker_id));
            active.lock_expires_at = Set(Some((Utc::now() + self.lock_duration).into()));
            active.status = Set(TaskStatus::Active.to_string());
            active.started_at = Set(Some(Utc::now().into()));
            let current_attempt = *active.attempt_count.as_ref();
            active.attempt_count = Set(current_attempt + 1);

            let updated = active.update(&txn).await?;

            txn.commit().await?;

            return Ok(Some(updated.into()));
        } else {
            txn.commit().await?;
        }

        Ok(None)
    }

    async fn mark_completed(&self, id: Uuid) -> Result<(), RepositoryError> {
        println!("DEBUG: mark_completed called for task {}", id);
        let task = self
            .find_by_id(id)
            .await?
            .ok_or(RepositoryError::NotFound)?;
        let mut updated_task = task.clone();
        updated_task.status = TaskStatus::Completed;
        updated_task.completed_at = Some(Utc::now().into());
        println!(
            "DEBUG: Updating task {} to status {:?}",
            id, updated_task.status
        );
        self.update(&updated_task).await?;
        println!("DEBUG: Successfully updated task {} to completed", id);
        Ok(())
    }

    async fn mark_failed(&self, id: Uuid) -> Result<(), RepositoryError> {
        let task = self
            .find_by_id(id)
            .await?
            .ok_or(RepositoryError::NotFound)?;
        let mut updated_task = task.clone();
        updated_task.status = TaskStatus::Failed;
        updated_task.completed_at = Some(Utc::now().into());
        self.update(&updated_task).await?;
        Ok(())
    }

    async fn mark_cancelled(&self, id: Uuid) -> Result<(), RepositoryError> {
        let task = self
            .find_by_id(id)
            .await?
            .ok_or(RepositoryError::NotFound)?;
        let mut updated_task = task.clone();
        updated_task.status = TaskStatus::Cancelled;
        updated_task.completed_at = Some(Utc::now().into());
        self.update(&updated_task).await?;
        Ok(())
    }

    async fn exists_by_url(&self, url: &str) -> Result<bool, RepositoryError> {
        let count = task_entity::Entity::find()
            .filter(task_entity::Column::Url.eq(url))
            .count(self.db.as_ref())
            .await?;
        Ok(count > 0)
    }

    async fn reset_stuck_tasks(&self, timeout: chrono::Duration) -> Result<u64, RepositoryError> {
        let threshold = Utc::now() - timeout;

        // Find tasks that are Active but lock_expires_at is past or started_at is too old
        // For simplicity, we use lock_expires_at if available, or a timeout based on started_at

        let result = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Queued.to_string()),
            )
            .col_expr(
                task_entity::Column::LockToken,
                Expr::value(Option::<Uuid>::None),
            )
            .col_expr(
                task_entity::Column::LockExpiresAt,
                Expr::value(Option::<DateTime<FixedOffset>>::None),
            )
            .col_expr(
                task_entity::Column::StartedAt,
                Expr::value(Option::<DateTime<FixedOffset>>::None),
            )
            .filter(task_entity::Column::Status.eq(TaskStatus::Active.to_string()))
            .filter(
                Condition::any()
                    .add(task_entity::Column::LockExpiresAt.lte(Utc::now()))
                    .add(
                        Condition::all()
                            .add(task_entity::Column::LockExpiresAt.is_null())
                            .add(task_entity::Column::StartedAt.lte(threshold)),
                    ),
            )
            .exec(self.db.as_ref())
            .await?;

        Ok(result.rows_affected)
    }

    async fn cancel_tasks_by_crawl_id(&self, crawl_id: Uuid) -> Result<u64, RepositoryError> {
        use sea_orm::sea_query::Expr;

        let result = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Cancelled.to_string()),
            )
            .col_expr(
                task_entity::Column::CompletedAt,
                Expr::value::<Option<DateTime<FixedOffset>>>(Some(Utc::now().fixed_offset())),
            )
            .filter(
                // 仅取消未完成的任务 (Queued 或 Active)
                task_entity::Column::Status.is_in(vec![
                    TaskStatus::Queued.to_string(),
                    TaskStatus::Active.to_string(),
                ]),
            )
            .filter(task_entity::Column::CrawlId.eq(crawl_id))
            .exec(self.db.as_ref())
            .await?;

        Ok(result.rows_affected)
    }

    async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
        // 将长时间处于队列状态的任务标记为失败
        // 使用24小时作为过期阈值
        let threshold = Utc::now() - chrono::Duration::hours(24);

        let result = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Failed.to_string()),
            )
            .col_expr(
                task_entity::Column::CompletedAt,
                Expr::value::<Option<DateTime<FixedOffset>>>(Some(Utc::now().fixed_offset())),
            )
            .filter(task_entity::Column::Status.eq(TaskStatus::Queued.to_string()))
            .filter(task_entity::Column::CreatedAt.lt(threshold))
            .exec(self.db.as_ref())
            .await?;

        Ok(result.rows_affected)
    }

    async fn find_by_crawl_id(&self, crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
        let models = task_entity::Entity::find()
            .filter(task_entity::Column::CrawlId.eq(crawl_id))
            .all(self.db.as_ref())
            .await?;

        Ok(models.into_iter().map(Task::from).collect())
    }

    async fn query_tasks(
        &self,
        params: TaskQueryParams,
    ) -> Result<(Vec<Task>, u64), RepositoryError> {
        println!("DEBUG: query_tasks params: {:?}", params);
        let mut query =
            task_entity::Entity::find().filter(task_entity::Column::TeamId.eq(params.team_id));

        // 应用任务ID过滤
        if let Some(ids) = params.task_ids {
            if !ids.is_empty() {
                query = query.filter(task_entity::Column::Id.is_in(ids));
            }
        }

        // 应用任务类型过滤
        if let Some(types) = params.task_types {
            if !types.is_empty() {
                let type_strings: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                query = query.filter(task_entity::Column::TaskType.is_in(type_strings));
            }
        }

        // 应用状态过滤
        if let Some(status_list) = params.statuses {
            if !status_list.is_empty() {
                let status_strings: Vec<String> =
                    status_list.iter().map(|s| s.to_string()).collect();
                println!("DEBUG: Filtering by status: {:?}", status_strings);
                query = query.filter(task_entity::Column::Status.is_in(status_strings));
            }
        }

        // 应用创建时间过滤
        if let Some(after) = params.created_after {
            query = query.filter(task_entity::Column::CreatedAt.gte(after));
        }
        if let Some(before) = params.created_before {
            query = query.filter(task_entity::Column::CreatedAt.lte(before));
        }

        // 应用爬取任务ID过滤
        if let Some(crawl_id_filter) = params.crawl_id {
            query = query.filter(task_entity::Column::CrawlId.eq(crawl_id_filter));
        }

        // 获取总数
        println!("DEBUG: About to count total with current filters");
        let total = query.clone().count(self.db.as_ref()).await?;
        println!("DEBUG: Total count result: {}", total);

        // 应用分页
        let models = query
            .order_by_desc(task_entity::Column::CreatedAt)
            .limit(params.limit as u64)
            .offset(params.offset as u64)
            .all(self.db.as_ref())
            .await?;

        let tasks: Vec<Task> = models.into_iter().map(Task::from).collect();
        Ok((tasks, total))
    }

    async fn batch_cancel(
        &self,
        task_ids: Vec<Uuid>,
        team_id: Uuid,
        force: bool,
    ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
        let mut cancelled_tasks = Vec::new();
        let mut failed_tasks = Vec::new();

        // 获取所有任务并验证团队权限
        let tasks = task_entity::Entity::find()
            .filter(task_entity::Column::Id.is_in(task_ids.clone()))
            .filter(task_entity::Column::TeamId.eq(team_id))
            .all(self.db.as_ref())
            .await?;

        // 创建任务ID到模型的映射
        let task_map: std::collections::HashMap<Uuid, task_entity::Model> =
            tasks.into_iter().map(|task| (task.id, task)).collect();

        // 收集需要级联取消的crawl_id
        let mut crawl_ids_to_cancel = Vec::new();

        for task_id in task_ids {
            if let Some(task_model) = task_map.get(&task_id) {
                let current_status =
                    TaskStatus::from_str(&task_model.status).unwrap_or(TaskStatus::Queued);

                println!("DEBUG: Processing task {} with status {:?}", task_id, current_status);

                match current_status {
                    TaskStatus::Queued => {
                        // 只有 Queued 状态可以直接取消
                        let result = task_entity::Entity::update_many()
                            .col_expr(
                                task_entity::Column::Status,
                                Expr::value(TaskStatus::Cancelled.to_string()),
                            )
                            .col_expr(
                                task_entity::Column::CompletedAt,
                                Expr::value::<Option<DateTime<FixedOffset>>>(Some(
                                    Utc::now().fixed_offset(),
                                )),
                            )
                            .filter(task_entity::Column::Id.eq(task_id))
                            .exec(self.db.as_ref())
                            .await?;

                        if result.rows_affected > 0 {
                            cancelled_tasks.push(task_id);

                            // 如果任务有crawl_id，收集用于级联取消
                            if let Some(crawl_id) = task_model.crawl_id {
                                crawl_ids_to_cancel.push(crawl_id);
                            }
                        } else {
                            failed_tasks
                                .push((task_id, "Failed to update task status".to_string()));
                        }
                    }
                    TaskStatus::Failed => {
                        failed_tasks.push((task_id, "Task is already failed".to_string()));
                    }
                    TaskStatus::Cancelled => {
                        failed_tasks.push((task_id, "Task is already cancelled".to_string()));
                    }
                    TaskStatus::Active => {
                        if force {
                            // 强制取消活跃任务
                            let result = task_entity::Entity::update_many()
                                .col_expr(
                                    task_entity::Column::Status,
                                    Expr::value(TaskStatus::Cancelled.to_string()),
                                )
                                .col_expr(
                                    task_entity::Column::CompletedAt,
                                    Expr::value::<Option<DateTime<FixedOffset>>>(Some(
                                        Utc::now().fixed_offset(),
                                    )),
                                )
                                .col_expr(
                                    task_entity::Column::LockToken,
                                    Expr::value(Option::<Uuid>::None),
                                )
                                .col_expr(
                                    task_entity::Column::LockExpiresAt,
                                    Expr::value(Option::<DateTime<FixedOffset>>::None),
                                )
                                .filter(task_entity::Column::Id.eq(task_id))
                                .exec(self.db.as_ref())
                                .await?;

                            if result.rows_affected > 0 {
                                cancelled_tasks.push(task_id);

                                // 如果任务有crawl_id，收集用于级联取消
                                if let Some(crawl_id) = task_model.crawl_id {
                                    crawl_ids_to_cancel.push(crawl_id);
                                }
                            } else {
                                failed_tasks
                                    .push((task_id, "Failed to update task status".to_string()));
                            }
                        } else {
                            failed_tasks.push((
                                task_id,
                                "Task is active, use force=true to cancel".to_string(),
                            ));
                        }
                    }
                    TaskStatus::Completed => {
                        failed_tasks.push((task_id, "Task is already completed".to_string()));
                    }
                }
            } else {
                failed_tasks.push((task_id, "Task not found or no permission".to_string()));
            }
        }

        // 执行级联取消：取消所有与已取消任务关联的爬取任务
        if !crawl_ids_to_cancel.is_empty() {
            // 去重crawl_id以避免重复取消
            crawl_ids_to_cancel.sort_unstable();
            crawl_ids_to_cancel.dedup();

            for crawl_id in crawl_ids_to_cancel {
                match self.cancel_tasks_by_crawl_id(crawl_id).await {
                    Ok(cancelled_count) => {
                        println!("Cancelled {} tasks for crawl_id: {}", cancelled_count, crawl_id);
                    }
                    Err(e) => {
                        eprintln!("Failed to cancel tasks for crawl_id {}: {}", crawl_id, e);
                    }
                }
            }
        }

        Ok((cancelled_tasks, failed_tasks))
    }
}
