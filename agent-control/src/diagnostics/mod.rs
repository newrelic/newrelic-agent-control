//! Diagnostics module for profiling and debugging Agent Control
//!
//! This module provides runtime diagnostics capabilities including:
//! - Spawn tracking to detect leaks and understand task lifecycle
//! - CPU profiling with flamegraphs
//! - Memory profiling with dhat
//! - Runtime metrics from tokio

pub mod global;
pub mod handlers;
pub mod metrics;
pub mod profiling;
pub mod spawn_tracker;

pub use profiling::ProfilingState;
pub use spawn_tracker::SpawnTracker;

/// Diagnostics configuration
#[derive(Debug, Clone)]
pub struct DiagnosticsConfig {
    /// Enable spawn tracking
    pub enable_spawn_tracking: bool,
    /// Enable profiling endpoints
    pub enable_profiling: bool,
    /// Maximum number of spawn records to keep
    pub max_spawn_records: usize,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            enable_spawn_tracking: cfg!(feature = "diagnostics"),
            enable_profiling: cfg!(feature = "diagnostics"),
            max_spawn_records: 10000,
        }
    }
}
