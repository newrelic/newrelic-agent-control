//! Global diagnostics singleton
//!
//! Provides a global spawn tracker that can be used throughout the application

use super::SpawnTracker;
use once_cell::sync::OnceCell;
use std::sync::Arc;

static GLOBAL_TRACKER: OnceCell<Arc<SpawnTracker>> = OnceCell::new();

/// Initialize the global spawn tracker
///
/// This should be called once at application startup
pub fn init_global_tracker(max_history: usize) -> Arc<SpawnTracker> {
    let tracker = Arc::new(SpawnTracker::new(max_history));
    GLOBAL_TRACKER
        .set(tracker.clone())
        .expect("Global tracker already initialized");
    tracing::info!("Global spawn tracker initialized");
    tracker
}

/// Get the global spawn tracker
///
/// Returns None if the tracker hasn't been initialized
pub fn global_tracker() -> Option<Arc<SpawnTracker>> {
    GLOBAL_TRACKER.get().cloned()
}

/// Helper macro to spawn using the global tracker
///
/// # Example
/// ```rust,ignore
/// use newrelic_agent_control::spawn_global;
///
/// spawn_global!("my-task", async {
///     // your async code
/// });
/// ```
#[macro_export]
macro_rules! spawn_global {
    ($name:expr, $future:expr) => {
        if let Some(tracker) = $crate::diagnostics::global::global_tracker() {
            tracker.spawn_tracked($name, file!(), line!(), $future)
        } else {
            tokio::spawn($future)
        }
    };
}
