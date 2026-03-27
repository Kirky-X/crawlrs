// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task state machine tests
//!
//! Comprehensive tests for the TaskStateMachine, state transitions,
//! and validation logic.

use chrono::Utc;
use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::workers::task_state_machine::{
    TaskStateError, TaskStateEvent, TaskStateMachine, TaskStateValidator,
};
use uuid::Uuid;

/// Helper function to create a test task with specified status
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

// === State Machine Creation Tests ===

#[test]
fn test_state_machine_creation_with_queued_status() {
    let task = create_test_task(TaskStatus::Queued);
    let state_machine = TaskStateMachine::new(task);

    assert_eq!(state_machine.current_status(), TaskStatus::Queued);
    assert_eq!(state_machine.task().status, TaskStatus::Queued);
}

#[test]
fn test_state_machine_creation_with_active_status() {
    let task = create_test_task(TaskStatus::Active);
    let state_machine = TaskStateMachine::new(task);

    assert_eq!(state_machine.current_status(), TaskStatus::Active);
}

#[test]
fn test_state_machine_creation_with_completed_status() {
    let task = create_test_task(TaskStatus::Completed);
    let state_machine = TaskStateMachine::new(task);

    assert_eq!(state_machine.current_status(), TaskStatus::Completed);
}

// === Valid State Transitions ===

#[test]
fn test_valid_queued_to_active_transition() {
    let task = create_test_task(TaskStatus::Queued);
    let mut state_machine = TaskStateMachine::new(task);

    let result = state_machine.handle_event(TaskStateEvent::Start);

    assert!(result.is_ok());
    assert_eq!(state_machine.current_status(), TaskStatus::Active);
    assert!(state_machine.task().started_at.is_some());
}

#[test]
fn test_valid_queued_to_cancelled_transition() {
    let task = create_test_task(TaskStatus::Queued);
    let mut state_machine = TaskStateMachine::new(task);

    let result = state_machine.handle_event(TaskStateEvent::Cancel);

    assert!(result.is_ok());
    assert_eq!(state_machine.current_status(), TaskStatus::Cancelled);
}

#[test]
fn test_valid_active_to_completed_transition() {
    let task = create_test_task(TaskStatus::Active);
    let mut state_machine = TaskStateMachine::new(task);

    let result = state_machine.handle_event(TaskStateEvent::Complete);

    assert!(result.is_ok());
    assert_eq!(state_machine.current_status(), TaskStatus::Completed);
    assert!(state_machine.task().completed_at.is_some());
}

#[test]
fn test_valid_active_to_failed_transition() {
    let task = create_test_task(TaskStatus::Active);
    let mut state_machine = TaskStateMachine::new(task);

    let result = state_machine.handle_event(TaskStateEvent::Fail);

    assert!(result.is_ok());
    assert_eq!(state_machine.current_status(), TaskStatus::Failed);
    assert_eq!(state_machine.task().retry_count, 1);
    assert_eq!(state_machine.task().attempt_count, 1);
}

#[test]
fn test_valid_active_to_cancelled_transition() {
    let task = create_test_task(TaskStatus::Active);
    let mut state_machine = TaskStateMachine::new(task);

    let result = state_machine.handle_event(TaskStateEvent::Cancel);

    assert!(result.is_ok());
    assert_eq!(state_machine.current_status(), TaskStatus::Cancelled);
}

#[test]
fn test_valid_failed_to_queued_transition_on_retry() {
    let task = create_test_task(TaskStatus::Failed);
    let mut state_machine = TaskStateMachine::new(task);

    let result = state_machine.handle_event(TaskStateEvent::Retry);

    assert!(result.is_ok());
    assert_eq!(state_machine.current_status(), TaskStatus::Queued);
}

// === Invalid State Transitions ===

#[test]
fn test_invalid_queued_to_completed_transition() {
    let task = create_test_task(TaskStatus::Queued);
    let mut state_machine = TaskStateMachine::new(task);

    let result = state_machine.handle_event(TaskStateEvent::Complete);

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        TaskStateError(_)
    ));
    assert_eq!(state_machine.current_status(), TaskStatus::Queued);
}

#[test]
fn test_invalid_queued_to_fail_transition() {
    let task = create_test_task(TaskStatus::Queued);
    let mut state_machine = TaskStateMachine::new(task);

    let result = state_machine.handle_event(TaskStateEvent::Fail);

    assert!(result.is_err());
    assert_eq!(state_machine.current_status(), TaskStatus::Queued);
}

#[test]
fn test_invalid_active_to_start_transition() {
    let task = create_test_task(TaskStatus::Active);
    let mut state_machine = TaskStateMachine::new(task);

    let result = state_machine.handle_event(TaskStateEvent::Start);

    assert!(result.is_err());
    assert_eq!(state_machine.current_status(), TaskStatus::Active);
}

#[test]
fn test_invalid_completed_state_transitions() {
    let task = create_test_task(TaskStatus::Completed);
    let mut state_machine = TaskStateMachine::new(task);

    // Cannot transition from terminal state
    assert!(state_machine.handle_event(TaskStateEvent::Start).is_err());
    assert!(state_machine.handle_event(TaskStateEvent::Fail).is_err());
    assert!(state_machine.handle_event(TaskStateEvent::Complete).is_err());
    assert!(state_machine.handle_event(TaskStateEvent::Cancel).is_err());
}

#[test]
fn test_invalid_cancelled_state_transitions() {
    let task = create_test_task(TaskStatus::Cancelled);
    let mut state_machine = TaskStateMachine::new(task);

    // Cannot transition from terminal state
    assert!(state_machine.handle_event(TaskStateEvent::Start).is_err());
    assert!(state_machine.handle_event(TaskStateEvent::Retry).is_err());
}

// === Retry and Attempt Count Tests ===

#[test]
fn test_retry_increments_count_on_fail() {
    let task = create_test_task(TaskStatus::Active);
    let mut state_machine = TaskStateMachine::new(task);

    assert_eq!(state_machine.task().retry_count, 0);
    assert_eq!(state_machine.task().attempt_count, 0);

    state_machine.handle_event(TaskStateEvent::Fail).unwrap();

    assert_eq!(state_machine.task().retry_count, 1);
    assert_eq!(state_machine.task().attempt_count, 1);
}

#[test]
fn test_active_retry_increments_count() {
    let task = create_test_task(TaskStatus::Active);
    let mut state_machine = TaskStateMachine::new(task);

    assert_eq!(state_machine.task().retry_count, 0);
    assert_eq!(state_machine.task().attempt_count, 0);

    state_machine.handle_event(TaskStateEvent::Retry).unwrap();

    assert_eq!(state_machine.task().retry_count, 1);
    assert_eq!(state_machine.task().attempt_count, 1);
    assert_eq!(state_machine.current_status(), TaskStatus::Active);
}

#[test]
fn test_multiple_failures_increment_count() {
    let task = create_test_task(TaskStatus::Active);
    let mut state_machine = TaskStateMachine::new(task);

    // First fail
    state_machine.handle_event(TaskStateEvent::Fail).unwrap();
    assert_eq!(state_machine.task().retry_count, 1);

    // Reset to queued
    state_machine.handle_event(TaskStateEvent::Retry).unwrap();

    // Second fail
    state_machine.handle_event(TaskStateEvent::Start).unwrap();
    state_machine.handle_event(TaskStateEvent::Fail).unwrap();

    assert_eq!(state_machine.task().retry_count, 2);
}

// === State Transition Validation Tests ===

#[test]
fn test_can_transition_valid_transitions() {
    let task = create_test_task(TaskStatus::Queued);
    let mut state_machine = TaskStateMachine::new(task);

    assert!(state_machine.can_transition(TaskStateEvent::Start));
    assert!(state_machine.can_transition(TaskStateEvent::Cancel));
    assert!(!state_machine.can_transition(TaskStateEvent::Complete));
    assert!(!state_machine.can_transition(TaskStateEvent::Fail));
}

#[test]
fn test_get_transition_description_valid() {
    let task = create_test_task(TaskStatus::Queued);
    let mut state_machine = TaskStateMachine::new(task);

    let description = state_machine.get_transition_description(TaskStateEvent::Start);

    assert!(description.contains("Queued"));
    assert!(description.contains("Active"));
}

#[test]
fn test_get_transition_description_invalid() {
    let task = create_test_task(TaskStatus::Completed);
    let mut state_machine = TaskStateMachine::new(task);

    let description = state_machine.get_transition_description(TaskStateEvent::Start);

    assert!(description.contains("Invalid"));
}

// === TaskStateValidator Tests ===

#[test]
fn test_state_validator_valid_transitions() {
    assert!(TaskStateValidator::is_valid_transition(
        TaskStatus::Queued,
        TaskStatus::Active
    ));

    assert!(TaskStateValidator::is_valid_transition(
        TaskStatus::Queued,
        TaskStatus::Cancelled
    ));

    assert!(TaskStateValidator::is_valid_transition(
        TaskStatus::Active,
        TaskStatus::Completed
    ));

    assert!(TaskStateValidator::is_valid_transition(
        TaskStatus::Active,
        TaskStatus::Failed
    ));

    assert!(TaskStateValidator::is_valid_transition(
        TaskStatus::Failed,
        TaskStatus::Queued
    ));
}

#[test]
fn test_state_validator_invalid_transitions() {
    assert!(!TaskStateValidator::is_valid_transition(
        TaskStatus::Queued,
        TaskStatus::Completed
    ));

    assert!(!TaskStateValidator::is_valid_transition(
        TaskStatus::Active,
        TaskStatus::Queued
    ));

    assert!(!TaskStateValidator::is_valid_transition(
        TaskStatus::Completed,
        TaskStatus::Active
    ));
}

#[test]
fn test_state_validator_valid_transitions_from_queued() {
    let transitions = TaskStateValidator::valid_transitions(TaskStatus::Queued);

    assert_eq!(transitions.len(), 2);
    assert!(transitions.contains(&TaskStatus::Active));
    assert!(transitions.contains(&TaskStatus::Cancelled));
}

#[test]
fn test_state_validator_valid_transitions_from_active() {
    let transitions = TaskStateValidator::valid_transitions(TaskStatus::Active);

    assert_eq!(transitions.len(), 3);
    assert!(transitions.contains(&TaskStatus::Completed));
    assert!(transitions.contains(&TaskStatus::Failed));
    assert!(transitions.contains(&TaskStatus::Cancelled));
}

#[test]
fn test_state_validator_valid_transitions_from_terminal_states() {
    let completed_transitions =
        TaskStateValidator::valid_transitions(TaskStatus::Completed);
    assert_eq!(completed_transitions.len(), 0);

    let cancelled_transitions =
        TaskStateValidator::valid_transitions(TaskStatus::Cancelled);
    assert_eq!(cancelled_transitions.len(), 0);

    let failed_transitions = TaskStateValidator::valid_transitions(TaskStatus::Failed);
    assert_eq!(failed_transitions.len(), 1);
    assert!(failed_transitions.contains(&TaskStatus::Queued));
}

#[test]
fn test_state_validator_is_terminal_state() {
    assert!(TaskStateValidator::is_terminal_state(TaskStatus::Completed));
    assert!(TaskStateValidator::is_terminal_state(TaskStatus::Failed));
    assert!(TaskStateValidator::is_terminal_state(TaskStatus::Cancelled));

    assert!(!TaskStateValidator::is_terminal_state(TaskStatus::Queued));
    assert!(!TaskStateValidator::is_terminal_state(TaskStatus::Active));
}

// === Task Access Tests ===

#[test]
fn test_state_machine_task_access() {
    let task = create_test_task(TaskStatus::Queued);
    let task_id = task.id;

    let state_machine = TaskStateMachine::new(task);

    assert_eq!(state_machine.task().id, task_id);
    assert_eq!(state_machine.task().status, TaskStatus::Queued);
}

#[test]
fn test_state_machine_task_mut_access() {
    let task = create_test_task(TaskStatus::Queued);
    let mut state_machine = TaskStateMachine::new(task);

    state_machine.task_mut().priority = 100;

    assert_eq!(state_machine.task().priority, 100);
}

// === Complex State Transition Workflows ===

#[test]
fn test_full_task_lifecycle() {
    let task = create_test_task(TaskStatus::Queued);
    let mut state_machine = TaskStateMachine::new(task);

    // Queued -> Active
    assert!(state_machine.handle_event(TaskStateEvent::Start).is_ok());
    assert_eq!(state_machine.current_status(), TaskStatus::Active);

    // Active -> Failed
    assert!(state_machine.handle_event(TaskStateEvent::Fail).is_ok());
    assert_eq!(state_machine.current_status(), TaskStatus::Failed);
    assert_eq!(state_machine.task().retry_count, 1);

    // Failed -> Queued (retry)
    assert!(state_machine.handle_event(TaskStateEvent::Retry).is_ok());
    assert_eq!(state_machine.current_status(), TaskStatus::Queued);

    // Queued -> Active (retry)
    assert!(state_machine.handle_event(TaskStateEvent::Start).is_ok());
    assert_eq!(state_machine.current_status(), TaskStatus::Active);

    // Active -> Completed
    assert!(state_machine.handle_event(TaskStateEvent::Complete).is_ok());
    assert_eq!(state_machine.current_status(), TaskStatus::Completed);
    assert!(state_machine.task().completed_at.is_some());
}

#[test]
fn test_task_cancellation_workflow() {
    let task = create_test_task(TaskStatus::Queued);
    let mut state_machine = TaskStateMachine::new(task);

    // Cancel from queued state
    assert!(state_machine.handle_event(TaskStateEvent::Cancel).is_ok());
    assert_eq!(state_machine.current_status(), TaskStatus::Cancelled);

    // Cannot transition from cancelled
    assert!(state_machine.handle_event(TaskStateEvent::Start).is_err());
}

#[test]
fn test_task_retry_until_max_retries() {
    let max_retries = 3;
    let mut task = create_test_task(TaskStatus::Active);
    task.max_retries = max_retries;

    let mut state_machine = TaskStateMachine::new(task);

    // Simulate multiple failures
    for i in 0..max_retries {
        // Active -> Failed
        assert!(state_machine.handle_event(TaskStateEvent::Fail).is_ok());
        assert_eq!(state_machine.current_status(), TaskStatus::Failed);
        assert_eq!(state_machine.task().retry_count, i + 1);

        // Failed -> Queued (retry)
        if i < max_retries - 1 {
            assert!(state_machine.handle_event(TaskStateEvent::Retry).is_ok());
            assert_eq!(state_machine.current_status(), TaskStatus::Queued);

            // Queued -> Active
            assert!(state_machine.handle_event(TaskStateEvent::Start).is_ok());
            assert_eq!(state_machine.current_status(), TaskStatus::Active);
        }
    }

    // After max retries, task should be in Failed state
    assert_eq!(state_machine.current_status(), TaskStatus::Failed);
    assert_eq!(state_machine.task().retry_count, max_retries);
}

// === Error Handling Tests ===

#[test]
fn test_state_error_display() {
    let task = create_test_task(TaskStatus::Queued);
    let mut state_machine = TaskStateMachine::new(task);

    let result = state_machine.handle_event(TaskStateEvent::Complete);

    assert!(result.is_err());
    let error_string = format!("{}", result.unwrap_err());
    assert!(error_string.contains("Invalid") || error_string.contains("not valid"));
}
