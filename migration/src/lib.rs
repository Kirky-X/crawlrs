// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

pub use sea_orm_migration::prelude::*;

mod m20251211_initial_schema;
mod m20251212_add_scrape_result_fields;
mod m20251213_add_missing_fields;
mod m20251213_add_task_fields;
mod m20251214_063323_m20251214_add_name_to_crawls;
mod m20251214_102356_add_url_to_crawls;
mod m20251214_103104_add_updated_at_to_crawls;
mod m20251214_103918_add_attempt_count_to_webhook_events;
mod m20251214_add_more_missing_fields;
mod m20251214_add_response_status_to_webhook_events;

/// 数据库迁移器
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    /// 获取所有迁移
    ///
    /// # 返回值
    ///
    /// 返回迁移列表
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20251211_initial_schema::Migration),
            Box::new(m20251212_add_scrape_result_fields::Migration),
            Box::new(m20251213_add_task_fields::Migration),
            Box::new(m20251213_add_missing_fields::Migration),
            Box::new(m20251214_add_more_missing_fields::Migration),
            Box::new(m20251214_add_response_status_to_webhook_events::Migration),
            Box::new(m20251214_063323_m20251214_add_name_to_crawls::Migration),
            Box::new(m20251214_102356_add_url_to_crawls::Migration),
            Box::new(m20251214_103104_add_updated_at_to_crawls::Migration),
            Box::new(m20251214_103918_add_attempt_count_to_webhook_events::Migration),
        ]
    }
}
