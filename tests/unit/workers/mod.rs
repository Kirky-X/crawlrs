// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Workers unit tests
//!
//! Unit tests for worker functions in src/workers/

use chrono::Utc;
use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::workers::task_state_machine::{TaskStateEvent, TaskStateMachine};
use uuid::Uuid;

fn create_test_task(status: TaskStatus) -> Task {
    Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status,
        priority: 0,
        team_id: Uuid::new_v4(),
        api_key_id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_state_machine_creation() {
        let task = create_test_task(TaskStatus::Queued);
        let state_machine = TaskStateMachine::new(task.clone());

        assert_eq!(state_machine.current_status(), TaskStatus::Queued);
        assert!(state_machine.can_transition_to(TaskStatus::Active));
    }

    #[test]
    fn test_valid_queued_transitions() {
        let task = create_test_task(TaskStatus::Queued);
        let mut state_machine = TaskStateMachine::new(task);

        // From Queued, we can go to Active, Cancelled
        assert!(state_machine.can_transition_to(TaskStatus::Active));
        assert!(state_machine.can_transition_to(TaskStatus::Cancelled));
        assert!(!state_machine.can_transition_to(TaskStatus::Completed));
    }

    #[test]
    fn test_valid_active_transitions() {
        let task = create_test_task(TaskStatus::Active);
        let mut state_machine = TaskStateMachine::new(task);

        // From Active, we can go to Completed, Failed, Cancelled
        assert!(state_machine.can_transition_to(TaskStatus::Completed));
        assert!(state_machine.can_transition_to(TaskStatus::Failed));
        assert!(state_machine.can_transition_to(TaskStatus::Cancelled));
    }

    #[test]
    fn test_terminal_states_no_transitions() {
        let completed_task = create_test_task(TaskStatus::Completed);
        let completed_sm = TaskStateMachine::new(completed_task);

        assert!(!completed_sm.can_transition_to(TaskStatus::Active));
        assert!(!completed_sm.can_transition_to(TaskStatus::Queued));

        let failed_task = create_test_task(TaskStatus::Failed);
        let failed_sm = TaskStateMachine::new(failed_task);

        assert!(!failed_sm.can_transition_to(TaskStatus::Active));
    }

    #[test]
    fn test_state_transition_events() {
        let task = create_test_task(TaskStatus::Queued);
        let mut state_machine = TaskStateMachine::new(task);

        // Test Start event
        let result = state_machine.handle_event(TaskStateEvent::Start);
        assert!(result.is_ok());
        assert_eq!(state_machine.current_status(), TaskStatus::Active);
    }

    #[test]
    fn test_retry_increments_count() {
        let task = create_test_task(TaskStatus::Failed);
        let mut state_machine = TaskStateMachine::new(task);

        // Retry should be valid from Failed
        let result = state_machine.handle_event(TaskStateEvent::Retry);
        assert!(result.is_ok());
    }

    #[test]
    fn test_retry_from_failed() {
        let task = create_test_task(TaskStatus::Failed);
        let mut state_machine = TaskStateMachine::new(task);

        // From Failed, we can retry to Queued
        assert!(state_machine.can_transition_to(TaskStatus::Queued));
    }

    #[test]
    fn test_state_validator() {
        let task = create_test_task(TaskStatus::Active);
        let state_machine = TaskStateMachine::new(task);

        // Verify state validation
        assert!(state_machine.is_valid_state());
        assert!(state_machine.current_status().is_active());
    }
}
