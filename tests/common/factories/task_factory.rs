// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 任务数据工厂
///
/// 提供测试任务创建的便捷函数

use crawlrs::domain::models::task::{TaskStatus, TaskType};
use crawlrs::infrastructure::database::entities::task::{self, Entity as TaskEntity};
use sea_orm::{DatabaseConnection, EntityTrait, Set};
use uuid::Uuid;

/// 任务工厂
pub struct TaskFactory;

impl TaskFactory {
    /// 创建抓取任务
    pub async fn create_scrape_task(
        db: &DatabaseConnection,
        url: &str,
    ) -> task::Model {
        let task_model = task::ActiveModel {
            id: Set(Uuid::new_v4()),
            url: Set(url.to_string()),
            task_type: Set(TaskType::Scrape.to_string()),
            status: Set(TaskStatus::Queued.to_string()),
            priority: Set(0),
            team_id: Set(Uuid::nil()),
            ..Default::default()
        };

        let result = TaskEntity::insert(task_model)
            .exec(db)
            .await
            .unwrap();

        // 获取插入的记录
        TaskEntity::find_by_id(result.last_insert_id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    /// 创建爬取任务
    pub async fn create_crawl_task(
        db: &DatabaseConnection,
        url: &str,
        depth: u32,
    ) -> task::Model {
        let task_model = task::ActiveModel {
            id: Set(Uuid::new_v4()),
            url: Set(url.to_string()),
            task_type: Set(TaskType::Crawl.to_string()),
            status: Set(TaskStatus::Queued.to_string()),
            priority: Set(0),
            team_id: Set(Uuid::nil()),
            payload: Set(serde_json::json!({
                "depth": depth
            })),
            ..Default::default()
        };

        let result = TaskEntity::insert(task_model)
            .exec(db)
            .await
            .unwrap();

        TaskEntity::find_by_id(result.last_insert_id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    /// 创建搜索任务
    pub async fn create_search_task(
        db: &DatabaseConnection,
        query: &str,
    ) -> task::Model {
        let task_model = task::ActiveModel {
            id: Set(Uuid::new_v4()),
            url: Set(format!("search:{}", query)),
            task_type: Set(TaskType::Scrape.to_string()),
            status: Set(TaskStatus::Queued.to_string()),
            priority: Set(0),
            team_id: Set(Uuid::nil()),
            payload: Set(serde_json::json!({
                "query": query
            })),
            ..Default::default()
        };

        let result = TaskEntity::insert(task_model)
            .exec(db)
            .await
            .unwrap();

        TaskEntity::find_by_id(result.last_insert_id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    /// 创建带团队的任务
    pub async fn create_task_with_team(
        db: &DatabaseConnection,
        url: &str,
        task_type: TaskType,
        team_id: Uuid,
    ) -> task::Model {
        let task_model = task::ActiveModel {
            id: Set(Uuid::new_v4()),
            url: Set(url.to_string()),
            task_type: Set(task_type.to_string()),
            status: Set(TaskStatus::Queued.to_string()),
            priority: Set(0),
            team_id: Set(team_id),
            ..Default::default()
        };

        let result = TaskEntity::insert(task_model)
            .exec(db)
            .await
            .unwrap();

        TaskEntity::find_by_id(result.last_insert_id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }
}