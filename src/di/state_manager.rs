// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Dependency state management module.
//!
//! This module provides functionality for tracking and managing the state
//! of all registered dependencies in the trait-kit DI container.

use std::collections::HashMap;
use std::sync::RwLock;
use thiserror::Error;

/// Component state enumeration
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ComponentState {
    /// Component is not yet initialized
    NotInitialized,
    /// Component is currently initializing
    Initializing,
    /// Component is ready and可用
    Ready,
    /// Component failed to initialize
    Failed(String),
}

/// Dependency state error
#[derive(Error, Debug)]
pub enum DependencyStateError {
    #[error("Component not found: {0}")]
    ComponentNotFound(String),
    #[error("Dependency cycle detected: {0}")]
    CycleDetected(String),
}

/// Dependency state information
#[derive(Debug, Clone)]
pub struct ComponentStateInfo {
    /// Component name
    pub name: String,
    /// Current state
    pub state: ComponentState,
    /// Initialization timestamp (if ready)
    pub initialized_at: Option<std::time::SystemTime>,
    /// Error message (if failed)
    pub error_message: Option<String>,
}

/// Dependency state manager for tracking component states
///
/// This manager tracks the initialization state of all components
/// registered in the DI container and provides methods for querying
/// and managing component states.
pub struct DependencyStateManager {
    /// Map of component names to their states
    states: RwLock<HashMap<String, ComponentStateInfo>>,
}

impl DependencyStateManager {
    /// Create a new DependencyStateManager
    pub fn new() -> Self {
        Self {
            states: RwLock::new(HashMap::new()),
        }
    }

    /// Register a component with the state manager
    pub fn register_component(&self, name: &str) {
        let mut states = match self.states.write() {
            Ok(guard) => guard,
            Err(e) => {
                log::error!(
                    "State manager RwLock poisoned during register_component: {}",
                    e
                );
                return;
            }
        };
        states.insert(
            name.to_string(),
            ComponentStateInfo {
                name: name.to_string(),
                state: ComponentState::NotInitialized,
                initialized_at: None,
                error_message: None,
            },
        );
    }

    /// Mark a component as initializing
    pub fn mark_initializing(&self, name: &str) {
        let mut states = match self.states.write() {
            Ok(guard) => guard,
            Err(e) => {
                log::error!(
                    "State manager RwLock poisoned during mark_initializing: {}",
                    e
                );
                return;
            }
        };
        if let Some(info) = states.get_mut(name) {
            info.state = ComponentState::Initializing;
        }
    }

    /// Mark a component as ready
    pub fn mark_ready(&self, name: &str) {
        let mut states = match self.states.write() {
            Ok(guard) => guard,
            Err(e) => {
                log::error!("State manager RwLock poisoned during mark_ready: {}", e);
                return;
            }
        };
        if let Some(info) = states.get_mut(name) {
            info.state = ComponentState::Ready;
            info.initialized_at = Some(std::time::SystemTime::now());
        }
    }

    /// Mark a component as failed
    pub fn mark_failed(&self, name: &str, error: &str) {
        let mut states = match self.states.write() {
            Ok(guard) => guard,
            Err(e) => {
                log::error!("State manager RwLock poisoned during mark_failed: {}", e);
                return;
            }
        };
        if let Some(info) = states.get_mut(name) {
            info.state = ComponentState::Failed(error.to_string());
            info.error_message = Some(error.to_string());
        }
    }

    /// Get the state of a specific component
    pub fn get_component_state(&self, name: &str) -> Option<ComponentStateInfo> {
        let states = match self.states.read() {
            Ok(guard) => guard,
            Err(e) => {
                log::error!(
                    "State manager RwLock poisoned during get_component_state: {}",
                    e
                );
                return None;
            }
        };
        states.get(name).cloned()
    }

    /// Get the states of all components
    pub fn get_all_states(&self) -> Vec<ComponentStateInfo> {
        let states = match self.states.read() {
            Ok(guard) => guard,
            Err(e) => {
                log::error!("State manager RwLock poisoned during get_all_states: {}", e);
                return Vec::new();
            }
        };
        states.values().cloned().collect()
    }

    /// Get components by state
    pub fn get_components_by_state(&self, state: ComponentState) -> Vec<ComponentStateInfo> {
        let states = match self.states.read() {
            Ok(guard) => guard,
            Err(e) => {
                log::error!(
                    "State manager RwLock poisoned during get_components_by_state: {}",
                    e
                );
                return Vec::new();
            }
        };
        states
            .values()
            .filter(|info| info.state == state)
            .cloned()
            .collect()
    }

    /// Check if all components are ready
    pub fn all_ready(&self) -> bool {
        let states = match self.states.read() {
            Ok(guard) => guard,
            Err(e) => {
                log::error!("State manager RwLock poisoned during all_ready: {}", e);
                return false;
            }
        };
        states
            .values()
            .all(|info| info.state == ComponentState::Ready)
    }

    /// Get count of components by state
    pub fn count_by_state(&self, state: &ComponentState) -> usize {
        let states = match self.states.read() {
            Ok(guard) => guard,
            Err(e) => {
                log::error!("State manager RwLock poisoned during count_by_state: {}", e);
                return 0;
            }
        };
        states.values().filter(|info| &info.state == state).count()
    }

    /// Get a summary of component states
    pub fn get_summary(&self) -> HashMap<ComponentState, usize> {
        let states = match self.states.read() {
            Ok(guard) => guard,
            Err(e) => {
                log::error!("State manager RwLock poisoned during get_summary: {}", e);
                return HashMap::new();
            }
        };
        let mut summary = HashMap::new();
        for info in states.values() {
            *summary.entry(info.state.clone()).or_insert(0) += 1;
        }
        summary
    }
}

impl Default for DependencyStateManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_creates_empty_manager() {
        let manager = DependencyStateManager::new();
        assert!(manager.get_all_states().is_empty());
    }

    #[test]
    fn test_default_equals_new() {
        let new_mgr = DependencyStateManager::new();
        let default_mgr = DependencyStateManager::default();
        assert!(new_mgr.get_all_states().is_empty());
        assert!(default_mgr.get_all_states().is_empty());
    }

    #[test]
    fn test_register_component_sets_not_initialized() {
        let manager = DependencyStateManager::new();
        manager.register_component("task_repo");

        let state = manager.get_component_state("task_repo");
        assert!(state.is_some());
        let info = state.unwrap();
        assert_eq!(info.name, "task_repo");
        assert_eq!(info.state, ComponentState::NotInitialized);
        assert!(info.initialized_at.is_none());
        assert!(info.error_message.is_none());
    }

    #[test]
    fn test_get_component_state_returns_none_for_unregistered() {
        let manager = DependencyStateManager::new();
        assert!(manager.get_component_state("nonexistent").is_none());
    }

    #[test]
    fn test_register_multiple_components() {
        let manager = DependencyStateManager::new();
        manager.register_component("comp_a");
        manager.register_component("comp_b");
        manager.register_component("comp_c");

        let all = manager.get_all_states();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_mark_initializing_changes_state() {
        let manager = DependencyStateManager::new();
        manager.register_component("redis");

        manager.mark_initializing("redis");
        let info = manager.get_component_state("redis").unwrap();
        assert_eq!(info.state, ComponentState::Initializing);
    }

    #[test]
    fn test_mark_initializing_unregistered_is_noop() {
        let manager = DependencyStateManager::new();
        manager.mark_initializing("nonexistent");
        assert!(manager.get_component_state("nonexistent").is_none());
    }

    #[test]
    fn test_mark_ready_sets_state_and_timestamp() {
        let manager = DependencyStateManager::new();
        manager.register_component("db");

        manager.mark_ready("db");
        let info = manager.get_component_state("db").unwrap();
        assert_eq!(info.state, ComponentState::Ready);
        assert!(info.initialized_at.is_some());
    }

    #[test]
    fn test_mark_ready_unregistered_is_noop() {
        let manager = DependencyStateManager::new();
        manager.mark_ready("nonexistent");
        assert!(manager.get_all_states().is_empty());
    }

    #[test]
    fn test_mark_failed_sets_error() {
        let manager = DependencyStateManager::new();
        manager.register_component("queue");

        let error_msg = "connection refused";
        manager.mark_failed("queue", error_msg);

        let info = manager.get_component_state("queue").unwrap();
        assert_eq!(info.state, ComponentState::Failed(error_msg.to_string()));
        assert_eq!(info.error_message, Some(error_msg.to_string()));
    }

    #[test]
    fn test_mark_failed_unregistered_is_noop() {
        let manager = DependencyStateManager::new();
        manager.mark_failed("nonexistent", "error");
        assert!(manager.get_all_states().is_empty());
    }

    #[test]
    fn test_full_lifecycle_not_initialized_to_ready() {
        let manager = DependencyStateManager::new();
        manager.register_component("service");

        assert_eq!(
            manager.get_component_state("service").unwrap().state,
            ComponentState::NotInitialized
        );

        manager.mark_initializing("service");
        assert_eq!(
            manager.get_component_state("service").unwrap().state,
            ComponentState::Initializing
        );

        manager.mark_ready("service");
        assert_eq!(
            manager.get_component_state("service").unwrap().state,
            ComponentState::Ready
        );
    }

    #[test]
    fn test_full_lifecycle_to_failed() {
        let manager = DependencyStateManager::new();
        manager.register_component("service");

        manager.mark_initializing("service");
        manager.mark_failed("service", "init failed");

        let info = manager.get_component_state("service").unwrap();
        assert_eq!(
            info.state,
            ComponentState::Failed("init failed".to_string())
        );
    }

    #[test]
    fn test_get_all_states_returns_all() {
        let manager = DependencyStateManager::new();
        manager.register_component("a");
        manager.register_component("b");

        let all = manager.get_all_states();
        assert_eq!(all.len(), 2);

        let names: Vec<String> = all.iter().map(|i| i.name.clone()).collect();
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
    }

    #[test]
    fn test_get_all_states_empty() {
        let manager = DependencyStateManager::new();
        assert!(manager.get_all_states().is_empty());
    }

    #[test]
    fn test_get_components_by_state() {
        let manager = DependencyStateManager::new();
        manager.register_component("ready_comp");
        manager.register_component("init_comp");
        manager.register_component("failed_comp");

        manager.mark_ready("ready_comp");
        manager.mark_initializing("init_comp");
        manager.mark_failed("failed_comp", "error");

        let ready = manager.get_components_by_state(ComponentState::Ready);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].name, "ready_comp");

        let initializing = manager.get_components_by_state(ComponentState::Initializing);
        assert_eq!(initializing.len(), 1);
        assert_eq!(initializing[0].name, "init_comp");

        let failed = manager.get_components_by_state(ComponentState::Failed("error".to_string()));
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].name, "failed_comp");

        let not_init = manager.get_components_by_state(ComponentState::NotInitialized);
        assert!(not_init.is_empty());
    }

    #[test]
    fn test_all_ready_empty_returns_true() {
        let manager = DependencyStateManager::new();
        assert!(manager.all_ready());
    }

    #[test]
    fn test_all_ready_all_ready() {
        let manager = DependencyStateManager::new();
        manager.register_component("a");
        manager.register_component("b");
        manager.mark_ready("a");
        manager.mark_ready("b");
        assert!(manager.all_ready());
    }

    #[test]
    fn test_all_ready_not_all_ready() {
        let manager = DependencyStateManager::new();
        manager.register_component("a");
        manager.register_component("b");
        manager.mark_ready("a");
        assert!(!manager.all_ready());
    }

    #[test]
    fn test_count_by_state() {
        let manager = DependencyStateManager::new();
        manager.register_component("a");
        manager.register_component("b");
        manager.register_component("c");

        manager.mark_ready("a");
        manager.mark_ready("b");
        manager.mark_initializing("c");

        assert_eq!(manager.count_by_state(&ComponentState::Ready), 2);
        assert_eq!(manager.count_by_state(&ComponentState::Initializing), 1);
        assert_eq!(manager.count_by_state(&ComponentState::NotInitialized), 0);
    }

    #[test]
    fn test_count_by_state_empty() {
        let manager = DependencyStateManager::new();
        assert_eq!(manager.count_by_state(&ComponentState::Ready), 0);
    }

    #[test]
    fn test_get_summary() {
        let manager = DependencyStateManager::new();
        manager.register_component("a");
        manager.register_component("b");
        manager.register_component("c");

        manager.mark_ready("a");
        manager.mark_ready("b");
        manager.mark_failed("c", "err");

        let summary = manager.get_summary();
        assert_eq!(summary.get(&ComponentState::Ready), Some(&2));
        assert_eq!(
            summary.get(&ComponentState::Failed("err".to_string())),
            Some(&1)
        );
        assert_eq!(summary.get(&ComponentState::NotInitialized), None);
    }

    #[test]
    fn test_get_summary_empty() {
        let manager = DependencyStateManager::new();
        let summary = manager.get_summary();
        assert!(summary.is_empty());
    }

    #[test]
    fn test_component_state_variants_equality() {
        assert_ne!(ComponentState::NotInitialized, ComponentState::Initializing);
        assert_ne!(ComponentState::NotInitialized, ComponentState::Ready);
        assert_ne!(
            ComponentState::NotInitialized,
            ComponentState::Failed("x".to_string())
        );
        assert_eq!(
            ComponentState::Failed("err".to_string()),
            ComponentState::Failed("err".to_string())
        );
        assert_ne!(
            ComponentState::Failed("err1".to_string()),
            ComponentState::Failed("err2".to_string())
        );
    }

    #[test]
    fn test_component_state_clone() {
        let state = ComponentState::Failed("error msg".to_string());
        let cloned = state.clone();
        assert_eq!(state, cloned);
    }

    #[test]
    fn test_component_state_info_clone() {
        let info = ComponentStateInfo {
            name: "test".to_string(),
            state: ComponentState::Ready,
            initialized_at: Some(std::time::SystemTime::now()),
            error_message: None,
        };
        let cloned = info.clone();
        assert_eq!(cloned.name, "test");
        assert_eq!(cloned.state, ComponentState::Ready);
        assert!(cloned.initialized_at.is_some());
    }

    #[test]
    fn test_component_state_debug_format() {
        let state = ComponentState::Ready;
        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("Ready"));

        let failed = ComponentState::Failed("boom".to_string());
        let debug_str = format!("{:?}", failed);
        assert!(debug_str.contains("Failed"));
        assert!(debug_str.contains("boom"));
    }

    #[test]
    fn test_dependency_state_error_display() {
        let err = DependencyStateError::ComponentNotFound("missing".to_string());
        assert!(err.to_string().contains("missing"));
        assert!(err.to_string().contains("not found"));

        let err2 = DependencyStateError::CycleDetected("a->b->a".to_string());
        assert!(err2.to_string().contains("cycle"));
        assert!(err2.to_string().contains("a->b->a"));
    }

    #[test]
    fn test_re_register_overwrites_state() {
        let manager = DependencyStateManager::new();
        manager.register_component("comp");
        manager.mark_ready("comp");

        // Re-register should reset to NotInitialized
        manager.register_component("comp");
        let info = manager.get_component_state("comp").unwrap();
        assert_eq!(info.state, ComponentState::NotInitialized);
        assert!(info.initialized_at.is_none());
    }
}
