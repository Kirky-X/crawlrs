// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task State Machine
//!
//! Implements the State pattern for managing task state transitions.
//! This provides a clean, type-safe way to handle task lifecycle management
//! with explicit state transitions and validation.
//!
//! # State Transition Diagram
//!
//! ```text
//! Queued  ──►  Active  ──►  Completed
//!    │          │            ▲
//!    │          │            │
//!    │          ▼            │
//!    │       Failed  ────────┤
//!    │          │            │
//!    │          ▼            │
//!    └─►  Cancelled ◄────────┘
//! ```
//!
//! # Usage Example
//!
//! ```rust
//! use crate::domain::models::{Task, TaskStatus, TaskType};
//! use crate::workers::task_state_machine::{TaskStateMachine, TaskStateEvent, TaskStateError};
//!
//! let task = Task::default();
//! let mut state_machine = TaskStateMachine::new(task);
//!
//! // Transition from Queued to Active
//! state_machine.handle_event(TaskStateEvent::Start)?;
//!
//! assert_eq!(state_machine.current_status(), TaskStatus::Active);
//! ```

use crate::domain::models::{Task, TaskStatus};
use chrono::Utc;

/// 任务状态机事件
///
/// 定义了可能触发状态转换的事件。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaskStateEvent {
    /// 开始执行任务
    Start,
    /// 完成任务
    Complete,
    /// 任务失败
    Fail,
    /// 取消任务
    Cancel,
    /// 重试任务
    Retry,
}

/// 任务状态机错误
#[derive(Debug, Clone, thiserror::Error)]
#[error(transparent)]
pub struct TaskStateError(#[from] TaskStateErrorKind);

/// 任务状态机错误类型
#[derive(Debug, Clone, thiserror::Error)]
pub enum TaskStateErrorKind {
    #[error("Invalid state transition from {from:?} to {to:?}")]
    InvalidTransition { from: TaskStatus, to: TaskStatus },

    #[error("Event {event:?} not valid for current state {status:?}")]
    EventNotValid {
        event: TaskStateEvent,
        status: TaskStatus,
    },
}

/// 任务状态机
///
/// 使用 State 模式管理任务的状态转换。
/// 确保所有状态转换都是有效的，并提供类型安全的转换接口。
#[derive(Debug, Clone)]
pub struct TaskStateMachine {
    task: Task,
}

impl TaskStateMachine {
    /// 创建新的任务状态机
    pub fn new(mut task: Task) -> Self {
        // 确保初始状态有效
        if task.status == TaskStatus::default() {
            task.status = TaskStatus::Queued;
        }
        Self { task }
    }

    /// 获取当前状态
    pub fn current_status(&self) -> TaskStatus {
        self.task.status
    }

    /// 获取任务引用
    pub fn task(&self) -> &Task {
        &self.task
    }

    /// 获取可变任务引用
    pub fn task_mut(&mut self) -> &mut Task {
        &mut self.task
    }

    /// 处理状态事件
    ///
    /// # Arguments
    ///
    /// * `event` - 要处理的事件
    ///
    /// # Returns
    ///
    /// 成功返回 Ok(()), 失败返回 TaskStateError
    pub fn handle_event(&mut self, event: TaskStateEvent) -> Result<(), TaskStateError> {
        let current_status = self.task.status;
        let new_status = self.calculate_next_status(current_status, event)?;

        // 更新状态和元数据
        self.task.status = new_status;
        self.task.updated_at = Utc::now().into();

        // 根据状态更新时间戳
        match new_status {
            TaskStatus::Active => {
                if self.task.started_at.is_none() {
                    self.task.started_at = Some(Utc::now().into());
                }
            }
            TaskStatus::Completed => {
                self.task.completed_at = Some(Utc::now().into());
            }
            _ => {}
        }

        Ok(())
    }

    /// 计算下一个状态
    ///
    /// 基于当前状态和事件计算目标状态。
    /// 如果转换无效，返回错误。
    fn calculate_next_status(
        &mut self,
        current: TaskStatus,
        event: TaskStateEvent,
    ) -> Result<TaskStatus, TaskStateErrorKind> {
        match (current, event) {
            // 有效转换: Queued -> Active
            (TaskStatus::Queued, TaskStateEvent::Start) => Ok(TaskStatus::Active),
            (TaskStatus::Queued, TaskStateEvent::Cancel) => Ok(TaskStatus::Cancelled),
            (TaskStatus::Queued, TaskStateEvent::Retry) => Ok(TaskStatus::Queued),
            // 无效转换: Queued 不能直接完成或失败
            (TaskStatus::Queued, TaskStateEvent::Complete)
            | (TaskStatus::Queued, TaskStateEvent::Fail) => {
                Err(TaskStateErrorKind::EventNotValid {
                    event,
                    status: current,
                })
            }

            // 有效转换: Active -> Completed/Failed/Cancelled
            (TaskStatus::Active, TaskStateEvent::Complete) => Ok(TaskStatus::Completed),
            (TaskStatus::Active, TaskStateEvent::Fail) => {
                self.increment_retry();
                Ok(TaskStatus::Failed)
            }
            (TaskStatus::Active, TaskStateEvent::Cancel) => Ok(TaskStatus::Cancelled),
            (TaskStatus::Active, TaskStateEvent::Retry) => {
                self.increment_retry();
                Ok(TaskStatus::Active)
            }
            // 无效转换: Active 不能再次开始
            (TaskStatus::Active, TaskStateEvent::Start) => Err(TaskStateErrorKind::EventNotValid {
                event,
                status: current,
            }),

            // 终态不允许转换
            (TaskStatus::Completed, _) => Err(TaskStateErrorKind::EventNotValid {
                event,
                status: current,
            }),
            (TaskStatus::Failed, _) => {
                // 允许从 Failed 状态重试
                if event == TaskStateEvent::Retry {
                    Ok(TaskStatus::Queued)
                } else {
                    Err(TaskStateErrorKind::EventNotValid {
                        event,
                        status: current,
                    })
                }
            }
            (TaskStatus::Cancelled, _) => Err(TaskStateErrorKind::EventNotValid {
                event,
                status: current,
            }),
        }
    }

    /// 增加重试计数
    fn increment_retry(&mut self) {
        self.task.retry_count += 1;
        self.task.attempt_count += 1;
    }

    /// 检查是否可以从当前状态转换到目标状态
    pub fn can_transition(&mut self, event: TaskStateEvent) -> bool {
        self.calculate_next_status(self.task.status, event).is_ok()
    }

    /// 获取状态转换描述
    pub fn get_transition_description(&mut self, event: TaskStateEvent) -> String {
        match self.calculate_next_status(self.task.status, event) {
            Ok(next) => format!("{:?} -> {:?}", self.task.status, next),
            Err(e) => format!("Invalid: {}", e),
        }
    }
}

/// 任务状态转换验证器
///
/// 提供静态的状态转换规则验证。
pub struct TaskStateValidator;

impl TaskStateValidator {
    /// 检查状态转换是否有效
    pub fn is_valid_transition(from: TaskStatus, to: TaskStatus) -> bool {
        matches!(
            (from, to),
            (TaskStatus::Queued, TaskStatus::Active)
                | (TaskStatus::Queued, TaskStatus::Cancelled)
                | (TaskStatus::Active, TaskStatus::Completed)
                | (TaskStatus::Active, TaskStatus::Failed)
                | (TaskStatus::Active, TaskStatus::Cancelled)
                | (TaskStatus::Failed, TaskStatus::Queued)
        )
    }

    /// 获取所有有效的转换
    pub fn valid_transitions(from: TaskStatus) -> Vec<TaskStatus> {
        match from {
            TaskStatus::Queued => vec![TaskStatus::Active, TaskStatus::Cancelled],
            TaskStatus::Active => vec![
                TaskStatus::Completed,
                TaskStatus::Failed,
                TaskStatus::Cancelled,
            ],
            TaskStatus::Completed => vec![],
            TaskStatus::Failed => vec![TaskStatus::Queued],
            TaskStatus::Cancelled => vec![],
        }
    }

    /// 检查是否为终态
    pub fn is_terminal_state(status: TaskStatus) -> bool {
        matches!(
            status,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::Task;

    fn create_test_task(status: TaskStatus) -> Task {
        Task {
            id: uuid::Uuid::new_v4(),
            task_type: crate::domain::models::TaskType::Scrape,
            status,
            priority: 0,
            team_id: uuid::Uuid::new_v4(),
            api_key_id: uuid::Uuid::new_v4(),
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

    #[test]
    fn test_valid_queued_transitions() {
        let task = create_test_task(TaskStatus::Queued);
        let mut state_machine = TaskStateMachine::new(task);

        assert!(state_machine.can_transition(TaskStateEvent::Start));
        assert!(state_machine.can_transition(TaskStateEvent::Cancel));
        assert!(!state_machine.can_transition(TaskStateEvent::Complete));
        assert!(!state_machine.can_transition(TaskStateEvent::Fail));

        state_machine.handle_event(TaskStateEvent::Start).unwrap();
        assert_eq!(state_machine.current_status(), TaskStatus::Active);
    }

    #[test]
    fn test_valid_active_transitions() {
        let task = create_test_task(TaskStatus::Active);
        let mut state_machine = TaskStateMachine::new(task);

        assert!(state_machine.can_transition(TaskStateEvent::Complete));
        assert!(state_machine.can_transition(TaskStateEvent::Fail));
        assert!(state_machine.can_transition(TaskStateEvent::Cancel));

        state_machine
            .handle_event(TaskStateEvent::Complete)
            .unwrap();
        assert_eq!(state_machine.current_status(), TaskStatus::Completed);
    }

    #[test]
    fn test_terminal_states_no_transitions() {
        let completed = create_test_task(TaskStatus::Completed);
        let mut completed_sm = TaskStateMachine::new(completed);
        assert!(!completed_sm.can_transition(TaskStateEvent::Start));
        assert!(!completed_sm.can_transition(TaskStateEvent::Fail));

        let cancelled = create_test_task(TaskStatus::Cancelled);
        let mut cancelled_sm = TaskStateMachine::new(cancelled);
        assert!(!cancelled_sm.can_transition(TaskStateEvent::Start));
    }

    #[test]
    fn test_retry_from_failed() {
        let task = create_test_task(TaskStatus::Failed);
        let mut state_machine = TaskStateMachine::new(task);

        assert!(state_machine.can_transition(TaskStateEvent::Retry));

        state_machine.handle_event(TaskStateEvent::Retry).unwrap();
        assert_eq!(state_machine.current_status(), TaskStatus::Queued);
    }

    #[test]
    fn test_state_validator() {
        assert!(TaskStateValidator::is_valid_transition(
            TaskStatus::Queued,
            TaskStatus::Active
        ));
        assert!(!TaskStateValidator::is_valid_transition(
            TaskStatus::Queued,
            TaskStatus::Completed
        ));
        assert!(TaskStateValidator::is_terminal_state(TaskStatus::Completed));
        assert!(!TaskStateValidator::is_terminal_state(TaskStatus::Active));
    }

    #[test]
    fn test_retry_increments_count() {
        let task = create_test_task(TaskStatus::Active);
        let mut state_machine = TaskStateMachine::new(task);

        assert_eq!(state_machine.task.retry_count, 0);
        assert_eq!(state_machine.task.attempt_count, 0);

        state_machine.handle_event(TaskStateEvent::Fail).unwrap();
        assert_eq!(state_machine.task.retry_count, 1);
    }
}
