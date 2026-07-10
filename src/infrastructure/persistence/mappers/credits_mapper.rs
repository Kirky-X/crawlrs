// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Credits Mapper - converts between Credits domain model and database entity

use crate::common::time_utils::{from_db_datetime, to_db_datetime};
use crate::domain::models::{Credits, CreditsTransaction, CreditsTransactionType};
use crate::infrastructure::database::entities::{credits, credits_transactions};

/// Mapper for converting between Credits domain model and database entity
pub struct CreditsMapper;

impl CreditsMapper {
    /// Convert database entity to domain model
    pub fn to_domain(entity: credits::Model) -> Credits {
        Credits::with_timestamps(
            entity.id,
            entity.team_id,
            entity.balance,
            from_db_datetime(entity.created_at),
            from_db_datetime(entity.updated_at),
        )
    }

    /// Convert domain model to database entity
    pub fn to_entity(domain: &Credits) -> credits::Model {
        credits::Model {
            id: domain.id,
            team_id: domain.team_id,
            balance: domain.balance(),
            created_at: to_db_datetime(domain.created_at),
            updated_at: to_db_datetime(domain.updated_at),
        }
    }

    /// Convert multiple entities to domain models
    pub fn to_domain_list(entities: Vec<credits::Model>) -> Vec<Credits> {
        entities.into_iter().map(Self::to_domain).collect()
    }
}

/// Mapper for converting between CreditsTransaction domain model and database entity
pub struct CreditsTransactionMapper;

impl CreditsTransactionMapper {
    /// Convert database entity to domain model
    pub fn to_domain(entity: credits_transactions::Model) -> CreditsTransaction {
        CreditsTransaction::with_timestamp(
            entity.id,
            entity.team_id,
            entity.amount,
            Self::parse_transaction_type(&entity.transaction_type),
            entity.description,
            entity.reference_id,
            from_db_datetime(entity.created_at),
        )
    }

    /// Convert domain model to database entity
    pub fn to_entity(domain: &CreditsTransaction) -> credits_transactions::Model {
        credits_transactions::Model {
            id: domain.id,
            team_id: domain.team_id,
            amount: domain.amount,
            transaction_type: domain.transaction_type.to_string(),
            description: domain.description.clone(),
            reference_id: domain.reference_id,
            created_at: to_db_datetime(domain.created_at),
        }
    }

    /// Convert multiple entities to domain models
    pub fn to_domain_list(entities: Vec<credits_transactions::Model>) -> Vec<CreditsTransaction> {
        entities.into_iter().map(Self::to_domain).collect()
    }

    /// Parse transaction type from string
    fn parse_transaction_type(s: &str) -> CreditsTransactionType {
        s.parse()
            .unwrap_or(CreditsTransactionType::ManualAdjustment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::time_utils::to_db_datetime;
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn test_credits_mapper_roundtrip() {
        let now = Utc::now();
        let domain = Credits::with_timestamps(Uuid::new_v4(), Uuid::new_v4(), 1000, now, now);

        let entity = CreditsMapper::to_entity(&domain);
        let back_to_domain = CreditsMapper::to_domain(entity);

        assert_eq!(domain.id, back_to_domain.id);
        assert_eq!(domain.team_id, back_to_domain.team_id);
        assert_eq!(domain.balance(), back_to_domain.balance());
    }

    #[test]
    fn test_transaction_mapper_roundtrip() {
        let now = Utc::now();
        let domain = CreditsTransaction::with_timestamp(
            Uuid::new_v4(),
            Uuid::new_v4(),
            -100,
            CreditsTransactionType::Scrape,
            "Test transaction".to_string(),
            Some(Uuid::new_v4()),
            now,
        );

        let entity = CreditsTransactionMapper::to_entity(&domain);
        let back_to_domain = CreditsTransactionMapper::to_domain(entity);

        assert_eq!(domain.id, back_to_domain.id);
        assert_eq!(domain.amount, back_to_domain.amount);
        assert_eq!(domain.transaction_type, back_to_domain.transaction_type);
    }

    #[test]
    fn test_credits_mapper_to_domain_list() {
        let now_db = to_db_datetime(Utc::now());
        let entities = vec![
            credits::Model {
                id: Uuid::new_v4(),
                team_id: Uuid::new_v4(),
                balance: 100,
                created_at: now_db,
                updated_at: now_db,
            },
            credits::Model {
                id: Uuid::new_v4(),
                team_id: Uuid::new_v4(),
                balance: 200,
                created_at: now_db,
                updated_at: now_db,
            },
        ];

        let domains = CreditsMapper::to_domain_list(entities);
        assert_eq!(domains.len(), 2);
        assert_eq!(domains[0].balance(), 100);
        assert_eq!(domains[1].balance(), 200);
    }

    #[test]
    fn test_credits_mapper_to_domain_list_empty() {
        let domains = CreditsMapper::to_domain_list(vec![]);
        assert!(domains.is_empty());
    }

    #[test]
    fn test_transaction_mapper_to_domain_list() {
        let now_db = to_db_datetime(Utc::now());
        let entities = vec![
            credits_transactions::Model {
                id: Uuid::new_v4(),
                team_id: Uuid::new_v4(),
                amount: 50,
                transaction_type: "subscription".to_string(),
                description: "Sub".to_string(),
                reference_id: None,
                created_at: now_db,
            },
            credits_transactions::Model {
                id: Uuid::new_v4(),
                team_id: Uuid::new_v4(),
                amount: -10,
                transaction_type: "scrape".to_string(),
                description: "Scrape".to_string(),
                reference_id: Some(Uuid::new_v4()),
                created_at: now_db,
            },
        ];

        let domains = CreditsTransactionMapper::to_domain_list(entities);
        assert_eq!(domains.len(), 2);
        assert_eq!(
            domains[0].transaction_type,
            CreditsTransactionType::Subscription
        );
        assert_eq!(domains[0].amount, 50);
        assert_eq!(domains[1].transaction_type, CreditsTransactionType::Scrape);
        assert_eq!(domains[1].amount, -10);
    }

    #[test]
    fn test_transaction_mapper_to_domain_list_empty() {
        let domains = CreditsTransactionMapper::to_domain_list(vec![]);
        assert!(domains.is_empty());
    }

    #[test]
    fn test_transaction_mapper_invalid_type_falls_back_to_manual_adjustment() {
        let now_db = to_db_datetime(Utc::now());
        let entity = credits_transactions::Model {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            amount: 100,
            transaction_type: "invalid_type".to_string(),
            description: "Bad type".to_string(),
            reference_id: None,
            created_at: now_db,
        };

        let domain = CreditsTransactionMapper::to_domain(entity);
        assert_eq!(
            domain.transaction_type,
            CreditsTransactionType::ManualAdjustment
        );
    }

    #[test]
    fn test_transaction_mapper_all_known_types_roundtrip() {
        let now = Utc::now();
        let types = vec![
            CreditsTransactionType::Search,
            CreditsTransactionType::Scrape,
            CreditsTransactionType::Extract,
            CreditsTransactionType::Crawl,
            CreditsTransactionType::ManualAdjustment,
            CreditsTransactionType::Subscription,
            CreditsTransactionType::Refund,
        ];

        for tx_type in types {
            let domain = CreditsTransaction::with_timestamp(
                Uuid::new_v4(),
                Uuid::new_v4(),
                100,
                tx_type,
                "Test".to_string(),
                None,
                now,
            );

            let entity = CreditsTransactionMapper::to_entity(&domain);
            let back_to_domain = CreditsTransactionMapper::to_domain(entity);
            assert_eq!(back_to_domain.transaction_type, tx_type);
        }
    }

    #[test]
    fn test_transaction_mapper_reference_id_none() {
        let now = Utc::now();
        let domain = CreditsTransaction::with_timestamp(
            Uuid::new_v4(),
            Uuid::new_v4(),
            50,
            CreditsTransactionType::Refund,
            "Refund".to_string(),
            None,
            now,
        );

        let entity = CreditsTransactionMapper::to_entity(&domain);
        assert_eq!(entity.reference_id, None);

        let back_to_domain = CreditsTransactionMapper::to_domain(entity);
        assert_eq!(back_to_domain.reference_id, None);
    }

    #[test]
    fn test_transaction_mapper_negative_amount() {
        let now = Utc::now();
        let domain = CreditsTransaction::with_timestamp(
            Uuid::new_v4(),
            Uuid::new_v4(),
            -500,
            CreditsTransactionType::Crawl,
            "Large debit".to_string(),
            Some(Uuid::new_v4()),
            now,
        );

        let entity = CreditsTransactionMapper::to_entity(&domain);
        assert_eq!(entity.amount, -500);

        let back_to_domain = CreditsTransactionMapper::to_domain(entity);
        assert_eq!(back_to_domain.amount, -500);
    }

    #[test]
    fn test_credits_mapper_negative_balance() {
        let now = Utc::now();
        let domain = Credits::with_timestamps(Uuid::new_v4(), Uuid::new_v4(), -50, now, now);

        let entity = CreditsMapper::to_entity(&domain);
        assert_eq!(entity.balance, -50);

        let back_to_domain = CreditsMapper::to_domain(entity);
        assert_eq!(back_to_domain.balance(), -50);
    }
}
