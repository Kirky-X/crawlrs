// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

use crate::domain::models::WebhookStatus;
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
