//! CPU profiling module
//!
//! Provides on-demand CPU profiling with flamegraph generation

use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Instant;

#[cfg(feature = "pprof")]
use pprof::ProfilerGuard;

#[cfg(feature = "pprof")]
use pprof::protos::Message;

/// State for managing profiling sessions
#[derive(Clone)]
pub struct ProfilingState {
    inner: Arc<RwLock<ProfilingStateInner>>,
}

struct ProfilingStateInner {
    #[cfg(feature = "pprof")]
    guard: Option<ProfilerGuard<'static>>,
    session_start: Option<Instant>,
    session_name: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProfilingInfo {
    pub active: bool,
    pub duration_secs: Option<f64>,
    pub session_name: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfilingError {
    #[error("Profiling is not enabled. Compile with --features pprof")]
    NotEnabled,
    #[error("Profiling session already active")]
    AlreadyActive,
    #[error("No active profiling session")]
    NotActive,
    #[error("Profiling error: {0}")]
    ProfilingFailed(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl ProfilingState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ProfilingStateInner {
                #[cfg(feature = "pprof")]
                guard: None,
                session_start: None,
                session_name: None,
            })),
        }
    }

    /// Start a CPU profiling session
    ///
    /// # Arguments
    /// * `frequency` - Sampling frequency in Hz (recommended: 100-1000)
    /// * `name` - Optional name for this profiling session
    pub fn start(&self, frequency: i32, name: Option<String>) -> Result<(), ProfilingError> {
        #[cfg(feature = "pprof")]
        {
            let mut inner = self.inner.write();

            if inner.guard.is_some() {
                return Err(ProfilingError::AlreadyActive);
            }

            let guard = pprof::ProfilerGuardBuilder::default()
                .frequency(frequency)
                .blocklist(&["libc", "libgcc", "pthread", "vdso"])
                .build()
                .map_err(|e| ProfilingError::ProfilingFailed(e.to_string()))?;

            inner.guard = Some(guard);
            inner.session_start = Some(Instant::now());
            inner.session_name = name.clone();

            tracing::info!(
                frequency = frequency,
                session_name = ?name,
                "CPU profiling started"
            );

            Ok(())
        }

        #[cfg(not(feature = "pprof"))]
        {
            let _ = (frequency, name);
            Err(ProfilingError::NotEnabled)
        }
    }

    /// Stop profiling and generate a flamegraph
    ///
    /// # Arguments
    /// * `output_path` - Path where to save the flamegraph SVG
    ///
    /// # Returns
    /// The path to the generated flamegraph
    pub fn stop_flamegraph(&self, output_path: &str) -> Result<String, ProfilingError> {
        #[cfg(feature = "pprof")]
        {
            let mut inner = self.inner.write();

            let guard = inner.guard.take().ok_or(ProfilingError::NotActive)?;
            let duration = inner.session_start.map(|start| start.elapsed());

            let report = guard
                .report()
                .build()
                .map_err(|e| ProfilingError::ProfilingFailed(e.to_string()))?;

            let file = std::fs::File::create(output_path)?;
            report
                .flamegraph(file)
                .map_err(|e| ProfilingError::ProfilingFailed(e.to_string()))?;

            tracing::info!(
                output_path = output_path,
                duration_secs = ?duration.map(|d| d.as_secs_f64()),
                "Flamegraph generated"
            );

            inner.session_start = None;
            inner.session_name = None;

            Ok(output_path.to_string())
        }

        #[cfg(not(feature = "pprof"))]
        {
            let _ = output_path;
            Err(ProfilingError::NotEnabled)
        }
    }

    /// Stop profiling and generate a pprof protobuf file
    ///
    /// # Arguments
    /// * `output_path` - Path where to save the pprof file
    ///
    /// # Returns
    /// The path to the generated pprof file
    pub fn stop_pprof(&self, output_path: &str) -> Result<String, ProfilingError> {
        #[cfg(feature = "pprof")]
        {
            let mut inner = self.inner.write();

            let guard = inner.guard.take().ok_or(ProfilingError::NotActive)?;

            let report = guard
                .report()
                .build()
                .map_err(|e| ProfilingError::ProfilingFailed(e.to_string()))?;

            let profile = report
                .pprof()
                .map_err(|e| ProfilingError::ProfilingFailed(e.to_string()))?;

            let mut content = Vec::new();
            profile
                .write_to_vec(&mut content)
                .map_err(|e| ProfilingError::ProfilingFailed(e.to_string()))?;

            use std::io::Write;
            let mut file = std::fs::File::create(output_path)?;
            file.write_all(&content)?;

            tracing::info!(output_path = output_path, "pprof file generated");

            inner.session_start = None;
            inner.session_name = None;

            Ok(output_path.to_string())
        }

        #[cfg(not(feature = "pprof"))]
        {
            let _ = output_path;
            Err(ProfilingError::NotEnabled)
        }
    }

    /// Get current profiling status
    pub fn status(&self) -> ProfilingInfo {
        let inner = self.inner.read();

        #[cfg(feature = "pprof")]
        let active = inner.guard.is_some();
        #[cfg(not(feature = "pprof"))]
        let active = false;

        let duration_secs = inner
            .session_start
            .map(|start| start.elapsed().as_secs_f64());

        ProfilingInfo {
            active,
            duration_secs,
            session_name: inner.session_name.clone(),
        }
    }
}

impl Default for ProfilingState {
    fn default() -> Self {
        Self::new()
    }
}
