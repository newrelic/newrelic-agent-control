//! OS-level command wrapper with not-started/started states, output redirection, and shutdown handling.

use super::error::CommandError;
use super::executable_data::ExecutableData;
use crate::agent_control::agent_id::AgentID;
use crate::sub_agent::on_host::managed_process::spawned_process::log_files::std_streams;
use std::path::PathBuf;
use std::process::{Child, Command};
#[cfg(target_family = "unix")]
use std::time::Duration;

////////////////////////////////////////////////////////////////////////////////////
// States for Started/Not Started/Sync Command
////////////////////////////////////////////////////////////////////////////////////
/// A configured but not-yet-spawned OS process.
pub struct OsProcessConfig {
    cmd: Command,
    agent_id: AgentID,
    maybe_log_dir: Option<PathBuf>,
    #[cfg(target_family = "unix")]
    shutdown_timeout: Duration,
}

/// A spawned OS process.
pub struct OsProcess {
    agent_id: AgentID,
    process: Child,
    #[cfg(target_family = "unix")]
    shutdown_timeout: Duration,
}

////////////////////////////////////////////////////////////////////////////////////
// Not Started OS Process
////////////////////////////////////////////////////////////////////////////////////
impl OsProcessConfig {
    /// Creates a not-started process config for the given executable. Its stdout/stderr are written
    /// to log files in `log_dir` when provided, and discarded otherwise.
    pub fn new(
        agent_id: AgentID,
        executable_data: &ExecutableData,
        maybe_log_dir: Option<PathBuf>,
    ) -> Self {
        let mut cmd = Command::new(&executable_data.bin);
        cmd.args(&executable_data.args).envs(&executable_data.env);

        Self {
            agent_id,
            cmd,
            maybe_log_dir,
            #[cfg(target_family = "unix")]
            shutdown_timeout: executable_data.shutdown_timeout,
        }
    }

    /// Spawns the process with its stdout/stderr set up.
    pub fn start(mut self) -> Result<OsProcess, CommandError> {
        let (stdout, stderr) = std_streams(&self.maybe_log_dir)?;
        let child = self.cmd.stdout(stdout).stderr(stderr).spawn()?;
        Ok(OsProcess {
            agent_id: self.agent_id,
            process: child,
            #[cfg(target_family = "unix")]
            shutdown_timeout: self.shutdown_timeout,
        })
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started OS Process
////////////////////////////////////////////////////////////////////////////////////

impl OsProcess {
    /// Returns the process id of the running process.
    pub fn get_pid(&self) -> u32 {
        self.process.id()
    }

    /// Returns the agent this process belongs to.
    pub(crate) fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }

    /// Returns how long to wait after a graceful shutdown before forcing a kill.
    #[cfg(target_family = "unix")]
    pub(crate) fn shutdown_timeout(&self) -> Duration {
        self.shutdown_timeout
    }

    /// Creates a Job Object and assigns this process to it.
    #[cfg(target_family = "windows")]
    pub(crate) fn create_job_object(
        &self,
    ) -> Result<crate::utils::job_object::JobObject, super::error::CommandError> {
        let job = crate::utils::job_object::JobObject::new()?;
        job.assign_process(&self.process)?;
        Ok(job)
    }
}

#[cfg(all(test, target_family = "unix"))]
mod tests {
    use super::*;
    use crate::agent_control::defaults::STDOUT_LOG_FILE_NAME;
    use crate::sub_agent::on_host::managed_process::spawned_process::restart_policy::RestartPolicy;
    use crate::utils::retry::retry;
    use std::collections::HashMap;

    #[test]
    fn process_keeps_writing_to_log_file_after_being_dropped() {
        let log_dir = tempfile::tempdir().unwrap();
        let exec_data = ExecutableData {
            id: "late-writer".to_string(),
            bin: "sh".to_string(),
            args: vec!["-c".to_string(), "sleep 1; echo late-output".to_string()],
            env: HashMap::new(),
            restart_policy: RestartPolicy::default(),
            shutdown_timeout: Default::default(),
        };

        let process = OsProcessConfig::new(
            AgentID::AgentControl,
            &exec_data,
            Some(log_dir.path().to_path_buf()),
        )
        .start()
        .unwrap();
        // Agent Control no longer holds any handle to the process or its log files.
        drop(process);

        let stdout_log = log_dir.path().join(STDOUT_LOG_FILE_NAME);
        retry(10, Duration::from_millis(500), || {
            let content = std::fs::read_to_string(&stdout_log).map_err(|e| e.to_string())?;
            content
                .contains("late-output")
                .then_some(())
                .ok_or_else(|| format!("process output was not written, got: {content:?}"))
        })
        .unwrap();
    }
}
