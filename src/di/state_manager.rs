// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Dependency state management module.
//!
//! This module provides functionality for tracking and managing the state
//! of all registered dependencies in the Shaku DI container.

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
                tracing::error!(
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
                tracing::error!(
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
                tracing::error!("State manager RwLock poisoned during mark_ready: {}", e);
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
                tracing::error!("State manager RwLock poisoned during mark_failed: {}", e);
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
                tracing::error!(
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
                tracing::error!("State manager RwLock poisoned during get_all_states: {}", e);
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
                tracing::error!(
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
                tracing::error!("State manager RwLock poisoned during all_ready: {}", e);
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
                tracing::error!("State manager RwLock poisoned during count_by_state: {}", e);
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
                tracing::error!("State manager RwLock poisoned during get_summary: {}", e);
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
