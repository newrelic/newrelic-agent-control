//! Spawn tracker to monitor async task lifecycle
//!
//! This module tracks all spawned tasks to help detect:
//! - Tasks that never complete (potential leaks)
//! - Tasks that are spawned but never polled
//! - Task creation/destruction patterns

use futures::FutureExt;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use tracing::{info, warn};
use uuid::Uuid;

/// Information about a spawned task
#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskInfo {
    /// Unique identifier for this task
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// When the task was spawned
    #[serde(skip)]
    pub spawned_at: Instant,
    /// Location in code where spawned (file:line)
    pub location: String,
    /// Current status
    pub status: TaskStatus,
    /// When the task completed (if applicable)
    #[serde(skip)]
    pub completed_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq)]
pub enum TaskStatus {
    /// Task is currently running
    Running,
    /// Task completed successfully
    Completed,
    /// Task was cancelled/dropped
    Cancelled,
    /// Task panicked
    Panicked,
}

impl TaskInfo {
    /// Get the duration the task has been running
    pub fn duration(&self) -> Duration {
        match self.completed_at {
            Some(completed) => completed.duration_since(self.spawned_at),
            None => Instant::now().duration_since(self.spawned_at),
        }
    }

    /// Check if task is a potential leak (running for too long)
    pub fn is_potential_leak(&self, threshold: Duration) -> bool {
        matches!(self.status, TaskStatus::Running) && self.duration() > threshold
    }
}

/// Global spawn tracker
#[derive(Clone, Debug)]
pub struct SpawnTracker {
    inner: Arc<RwLock<SpawnTrackerInner>>,
}

#[derive(Debug)]
struct SpawnTrackerInner {
    /// Active tasks currently running
    active_tasks: HashMap<String, TaskInfo>,
    /// Completed tasks (kept for history)
    completed_tasks: Vec<TaskInfo>,
    /// Maximum number of completed tasks to keep
    max_history: usize,
    /// Total tasks spawned (for stats)
    total_spawned: u64,
    /// Total tasks completed (for stats)
    total_completed: u64,
}

impl SpawnTracker {
    /// Create a new spawn tracker
    pub fn new(max_history: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(SpawnTrackerInner {
                active_tasks: HashMap::new(),
                completed_tasks: Vec::new(),
                max_history,
                total_spawned: 0,
                total_completed: 0,
            })),
        }
    }

    /// Spawn a tracked task with location tracking
    ///
    /// # Example
    /// ```rust,ignore
    /// let tracker = SpawnTracker::new(1000);
    /// let handle = tracker.spawn_tracked(
    ///     "my-task",
    ///     file!(),
    ///     line!(),
    ///     async {
    ///         // your task code
    ///     }
    /// );
    /// ```
    pub fn spawn_tracked<F>(
        &self,
        name: &str,
        file: &str,
        line: u32,
        future: F,
    ) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let task_id = Uuid::new_v4().to_string();
        let location = format!("{}:{}", file, line);

        let task_info = TaskInfo {
            id: task_id.clone(),
            name: name.to_string(),
            spawned_at: Instant::now(),
            location: location.clone(),
            status: TaskStatus::Running,
            completed_at: None,
        };

        // Register the spawn
        {
            let mut inner = self.inner.write();
            inner.active_tasks.insert(task_id.clone(), task_info);
            inner.total_spawned += 1;
        }

        info!(
            task_id = %task_id,
            task_name = %name,
            location = %location,
            "Task spawned"
        );

        let tracker = self.clone();
        let name = name.to_string();

        tokio::spawn(async move {
            let result = std::panic::AssertUnwindSafe(future).catch_unwind().await;

            match result {
                Ok(output) => {
                    tracker.mark_completed(&task_id, TaskStatus::Completed);
                    info!(task_id = %task_id, task_name = %name, "Task completed successfully");
                    output
                }
                Err(panic_info) => {
                    tracker.mark_completed(&task_id, TaskStatus::Panicked);
                    warn!(
                        task_id = %task_id,
                        task_name = %name,
                        "Task panicked"
                    );
                    std::panic::resume_unwind(panic_info);
                }
            }
        })
    }

    /// Mark a task as completed
    fn mark_completed(&self, task_id: &str, status: TaskStatus) {
        let mut inner = self.inner.write();

        if let Some(mut task_info) = inner.active_tasks.remove(task_id) {
            task_info.status = status;
            task_info.completed_at = Some(Instant::now());

            inner.total_completed += 1;
            inner.completed_tasks.push(task_info);

            // Trim history if needed
            if inner.completed_tasks.len() > inner.max_history {
                let excess = inner.completed_tasks.len() - inner.max_history;
                inner.completed_tasks.drain(0..excess);
            }
        }
    }

    /// Get all currently active tasks
    pub fn active_tasks(&self) -> Vec<TaskInfo> {
        let inner = self.inner.read();
        inner.active_tasks.values().cloned().collect()
    }

    /// Get recently completed tasks
    pub fn completed_tasks(&self, limit: usize) -> Vec<TaskInfo> {
        let inner = self.inner.read();
        inner
            .completed_tasks
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    /// Get tasks that might be leaking (running longer than threshold)
    pub fn potential_leaks(&self, threshold: Duration) -> Vec<TaskInfo> {
        let inner = self.inner.read();
        inner
            .active_tasks
            .values()
            .filter(|task| task.is_potential_leak(threshold))
            .cloned()
            .collect()
    }

    /// Get statistics
    pub fn stats(&self) -> SpawnStats {
        let inner = self.inner.read();
        SpawnStats {
            total_spawned: inner.total_spawned,
            total_completed: inner.total_completed,
            currently_active: inner.active_tasks.len(),
            completed_history_size: inner.completed_tasks.len(),
        }
    }

    /// Clear all completed tasks from history
    pub fn clear_history(&self) {
        let mut inner = self.inner.write();
        inner.completed_tasks.clear();
    }
}

/// Statistics about spawned tasks
#[derive(Debug, Clone, serde::Serialize)]
pub struct SpawnStats {
    /// Total tasks spawned since start
    pub total_spawned: u64,
    /// Total tasks completed since start
    pub total_completed: u64,
    /// Currently active tasks
    pub currently_active: usize,
    /// Number of completed tasks in history
    pub completed_history_size: usize,
}

/// Helper macro to spawn tracked tasks with automatic location capture
#[macro_export]
macro_rules! spawn_tracked {
    ($tracker:expr, $name:expr, $future:expr) => {
        $tracker.spawn_tracked($name, file!(), line!(), $future)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_spawn_tracking() {
        let tracker = SpawnTracker::new(100);

        let handle = tracker.spawn_tracked("test-task", file!(), line!(), async {
            sleep(Duration::from_millis(10)).await;
            42
        });

        // Task should be active
        assert_eq!(tracker.active_tasks().len(), 1);

        let result = handle.await.unwrap();
        assert_eq!(result, 42);

        // Give it a moment to update
        sleep(Duration::from_millis(10)).await;

        // Task should be completed
        assert_eq!(tracker.active_tasks().len(), 0);
        assert_eq!(tracker.completed_tasks(10).len(), 1);

        let stats = tracker.stats();
        assert_eq!(stats.total_spawned, 1);
        assert_eq!(stats.total_completed, 1);
    }

    #[tokio::test]
    async fn test_leak_detection() {
        let tracker = SpawnTracker::new(100);

        // Spawn a long-running task
        let _handle = tracker.spawn_tracked("long-task", file!(), line!(), async {
            sleep(Duration::from_secs(100)).await;
        });

        sleep(Duration::from_millis(100)).await;

        // Should detect as potential leak
        let leaks = tracker.potential_leaks(Duration::from_millis(50));
        assert_eq!(leaks.len(), 1);
        assert_eq!(leaks[0].name, "long-task");
    }
}
