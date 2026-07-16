//! Periodic CPU/memory sampling for Agent Control and its on-host sub-agents' processes.
//!
//! Scoped to on-host only: k8s sub-agents run as separate pods, not child processes of AC, and
//! getting their resource usage would need the Kubernetes Metrics API (which needs
//! `metrics-server` installed in the cluster - not something we can assume exists in every
//! customer cluster). AC's own resource usage works uniformly on host-linux/host-windows/k8s
//! since a process's own PID is always locally readable, cgroup-scoped correctly by the kernel
//! even inside a container.

use crate::event::cancellation::CancellationMessage;
use crate::event::channel::EventConsumer;
use crate::instrumentation::metrics;
use crate::utils::thread_context::{NotStartedThreadContext, StartedThreadContext};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tracing::debug;

pub const RESOURCE_SAMPLER_THREAD_NAME: &str = "resource_sampler";

/// Refreshes only CPU and memory for the processes we sample - no need for the other
/// process-level data (cwd, exe path, environment, etc.) sysinfo can optionally collect.
fn refresh_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing().with_cpu().with_memory()
}

/// Samples CPU/memory for a single process, identified by PID, and records it via
/// [`metrics::record_process_resources`]. Keeps its own [`System`] instance so sysinfo's
/// internal per-PID CPU delta tracking persists across calls - CPU usage is computed as a delta
/// since the previous refresh for that PID, so the first sample after a process (re)starts will
/// read as 0. That's expected and self-corrects on the next tick; not worth a synthetic
/// double-refresh to "fix" for a periodic gauge.
pub struct ResourceSampler {
    system: System,
}

impl ResourceSampler {
    pub fn new() -> Self {
        Self {
            system: System::new(),
        }
    }

    /// Refreshes and records CPU/memory for `pid`. A no-op (with a debug log) if the process
    /// isn't found - e.g. it exited between the caller reading the PID and this call running.
    pub fn sample(&mut self, pid: u32, agent_id: &str, agent_type: &str) {
        let sysinfo_pid = Pid::from_u32(pid);
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[sysinfo_pid]),
            true,
            refresh_kind(),
        );

        let Some(process) = self.system.process(sysinfo_pid) else {
            debug!(%agent_id, pid, "Process not found while sampling resource usage");
            return;
        };

        metrics::record_process_resources(
            agent_id,
            agent_type,
            process.cpu_usage() as f64,
            process.memory(),
        );
    }
}

impl Default for ResourceSampler {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawns a thread that periodically samples CPU/memory for a fixed PID (e.g. Agent Control's
/// own process) and records it. Use [`spawn_resource_sampler_for_shared_pid`] instead when the
/// PID can change over the sampled process's lifetime (e.g. an on-host sub-agent that gets
/// restarted).
pub fn spawn_resource_sampler(
    agent_id: String,
    agent_type: String,
    pid: u32,
    interval: Duration,
) -> StartedThreadContext {
    spawn_resource_sampler_for_shared_pid(
        agent_id,
        agent_type,
        Arc::new(Mutex::new(Some(pid))),
        interval,
    )
}

/// Spawns a thread that periodically samples CPU/memory for whatever PID is currently held in
/// `current_pid`, skipping the tick if it's `None` (e.g. the process is mid-restart). Used for
/// on-host sub-agents, whose PID changes across restarts within the same executable's lifetime -
/// the owning supervisor thread updates `current_pid` on each spawn/exit.
pub fn spawn_resource_sampler_for_shared_pid(
    agent_id: String,
    agent_type: String,
    current_pid: Arc<Mutex<Option<u32>>>,
    interval: Duration,
) -> StartedThreadContext {
    let callback = move |stop_consumer: EventConsumer<CancellationMessage>| {
        let mut sampler = ResourceSampler::new();
        loop {
            // A poisoned lock still holds a valid `Option<u32>` - recovering it is safe and
            // keeps the sampler running instead of taking down the sampler thread over a panic
            // that happened elsewhere while holding this trivial lock.
            if let Some(pid) = *current_pid.lock().unwrap_or_else(|e| e.into_inner()) {
                sampler.sample(pid, &agent_id, &agent_type);
            }

            if stop_consumer.is_cancelled_with_timeout(interval) {
                break;
            }
        }
    };
    NotStartedThreadContext::new(RESOURCE_SAMPLER_THREAD_NAME, callback).start()
}
