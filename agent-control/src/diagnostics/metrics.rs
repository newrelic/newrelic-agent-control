//! Runtime metrics module
//!
//! Provides access to Tokio runtime metrics and system information

use serde::Serialize;
use std::time::Duration;

/// Runtime metrics snapshot
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeMetrics {
    /// Number of worker threads in the runtime
    pub num_workers: usize,
    /// Tokio runtime metrics (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokio_metrics: Option<TokioMetrics>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokioMetrics {
    /// Total time the runtime has been alive
    pub uptime_secs: f64,
    /// Number of times worker threads parked
    pub worker_park_count: u64,
}

/// Memory usage information
#[derive(Debug, Clone, Serialize)]
pub struct MemoryMetrics {
    /// Physical memory usage in bytes (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_mem_bytes: Option<usize>,
    /// Virtual memory usage in bytes (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virtual_mem_bytes: Option<usize>,
}

/// Get current runtime metrics
pub fn get_runtime_metrics() -> RuntimeMetrics {
    let handle = tokio::runtime::Handle::current();
    let metrics = handle.metrics();

    RuntimeMetrics {
        num_workers: metrics.num_workers(),
        tokio_metrics: get_tokio_detailed_metrics(&metrics),
    }
}

fn get_tokio_detailed_metrics(metrics: &tokio::runtime::RuntimeMetrics) -> Option<TokioMetrics> {
    // Get detailed metrics for worker 0 as a representative sample
    let worker_park_count = metrics.worker_park_count(0);

    Some(TokioMetrics {
        uptime_secs: 0.0, // Could be tracked separately if needed
        worker_park_count,
    })
}

/// Get memory usage metrics
pub fn get_memory_metrics() -> MemoryMetrics {
    #[cfg(feature = "memory-stats")]
    {
        if let Some(usage) = memory_stats::memory_stats() {
            return MemoryMetrics {
                physical_mem_bytes: Some(usage.physical_mem),
                virtual_mem_bytes: Some(usage.virtual_mem),
            };
        }
    }

    MemoryMetrics {
        physical_mem_bytes: None,
        virtual_mem_bytes: None,
    }
}

/// Complete diagnostics snapshot
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsSnapshot {
    /// Timestamp when snapshot was taken
    pub timestamp: String,
    /// Runtime metrics
    pub runtime: RuntimeMetrics,
    /// Memory metrics
    pub memory: MemoryMetrics,
    /// Spawn statistics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawn_stats: Option<crate::diagnostics::spawn_tracker::SpawnStats>,
}

impl DiagnosticsSnapshot {
    pub fn capture(spawn_tracker: Option<&crate::diagnostics::SpawnTracker>) -> Self {
        Self {
            timestamp: chrono::Utc::now().to_rfc3339(),
            runtime: get_runtime_metrics(),
            memory: get_memory_metrics(),
            spawn_stats: spawn_tracker.map(|tracker| tracker.stats()),
        }
    }
}

/// Helper to format duration in a human-readable way
pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// Helper to format bytes in a human-readable way
pub fn format_bytes(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = KB * 1024;
    const GB: usize = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
