// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(dead_code)]

use crawlrs::domain::models::task_domain::{TaskStatus, TaskType};
use crawlrs::infrastructure::database::entities::task::{self, Entity as TaskEntity};
use sea_orm::{DatabaseConnection, EntityTrait, Set};
use uuid::Uuid;

pub struct TaskFactory;

impl TaskFactory {
    async fn insert_and_fetch(db: &DatabaseConnection, model: task::ActiveModel) -> task::Model {
        let result = TaskEntity::insert(model)
            .exec(db)
            .await
            .expect("Failed to insert task into database");
        TaskEntity::find_by_id(result.last_insert_id)
            .one(db)
            .await
            .expect("Failed to query task from database")
            .expect("Task not found after insertion")
    }

    fn base_task_model(url: String, task_type: TaskType, team_id: Uuid) -> task::ActiveModel {
        task::ActiveModel {
            id: Set(Uuid::new_v4()),
            url: Set(url),
            task_type: Set(task_type.to_string()),
            status: Set(TaskStatus::Queued.to_string()),
            priority: Set(0),
            team_id: Set(team_id),
            ..Default::default()
        }
    }

    pub async fn create_scrape_task(db: &DatabaseConnection, url: &str) -> task::Model {
        let model = Self::base_task_model(url.to_string(), TaskType::Scrape, Uuid::nil());
        Self::insert_and_fetch(db, model).await
    }

    pub async fn create_crawl_task(db: &DatabaseConnection, url: &str, depth: u32) -> task::Model {
        let mut model = Self::base_task_model(url.to_string(), TaskType::Crawl, Uuid::nil());
        model.task_type = Set(TaskType::Crawl.to_string());
        model.payload = Set(serde_json::json!({ "depth": depth }));
        Self::insert_and_fetch(db, model).await
    }

    pub async fn create_search_task(db: &DatabaseConnection, query: &str) -> task::Model {
        let mut model =
            Self::base_task_model(format!("search:{}", query), TaskType::Scrape, Uuid::nil());
        model.payload = Set(serde_json::json!({ "query": query }));
        Self::insert_and_fetch(db, model).await
    }

    pub async fn create_task_with_team(
        db: &DatabaseConnection,
        url: &str,
        task_type: TaskType,
        team_id: Uuid,
    ) -> task::Model {
        let model = Self::base_task_model(url.to_string(), task_type, team_id);
        Self::insert_and_fetch(db, model).await
    }
}
