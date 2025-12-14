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

use crate::domain::models::webhook::WebhookStatus;
use sea_orm::entity::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(
    rs_type = "String",
    db_type = "String(Some(20))",
    enum_name = "webhook_status"
)]
pub enum SeaWebhookStatus {
    #[sea_orm(string_value = "pending")]
    Pending,
    #[sea_orm(string_value = "delivered")]
    Delivered,
    #[sea_orm(string_value = "failed")]
    Failed,
    #[sea_orm(string_value = "dead")]
    Dead,
}

impl From<WebhookStatus> for SeaWebhookStatus {
    fn from(status: WebhookStatus) -> Self {
        match status {
            WebhookStatus::Pending => SeaWebhookStatus::Pending,
            WebhookStatus::Delivered => SeaWebhookStatus::Delivered,
            WebhookStatus::Failed => SeaWebhookStatus::Failed,
            WebhookStatus::Dead => SeaWebhookStatus::Dead,
        }
    }
}

impl From<SeaWebhookStatus> for WebhookStatus {
    fn from(status: SeaWebhookStatus) -> Self {
        match status {
            SeaWebhookStatus::Pending => WebhookStatus::Pending,
            SeaWebhookStatus::Delivered => WebhookStatus::Delivered,
            SeaWebhookStatus::Failed => WebhookStatus::Failed,
            SeaWebhookStatus::Dead => WebhookStatus::Dead,
        }
    }
}
