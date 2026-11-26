//! OS-level command wrapper with not-started/started states, output redirection, and shutdown handling.

use super::error::CommandError;
use super::executable_data::ExecutableData;
use crate::agent_control::agent_id::AgentID;
use crate::sub_agent::on_host::managed_process::spawned_process::log_files::std_streams;
use crate::sub_agent::on_host::process_group_tracker::TrackedProcessGroup;
#[cfg(target_family = "unix")]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command};
#[cfg(target_family = "unix")]
use std::time::Duration;

////////////////////////////////////////////////////////////////////////////////////
// States for Started/Not Started/Sync Command
////////////////////////////////////////////////////////////////////////////////////
/// A configured but not-yet-spawned OS process.
pub struct OsProcessConfig<G> {
    cmd: Command,
    agent_id: AgentID,
    maybe_log_dir: Option<PathBuf>,
    #[cfg(target_family = "unix")]
    shutdown_timeout: Duration,
    group: G,
}

/// A spawned OS process.
pub struct OsProcess<G> {
    agent_id: AgentID,
    process: Child,
    #[cfg(target_family = "unix")]
    shutdown_timeout: Duration,
    group: G,
}

////////////////////////////////////////////////////////////////////////////////////
// Not Started OS Process
////////////////////////////////////////////////////////////////////////////////////
impl<G> OsProcessConfig<G> {
    /// Creates a not-started process config for the given executable, which will run inside the
    /// `group`. Its stdout/stderr are written to log files in `log_dir` when provided, and
    /// discarded otherwise.
    pub fn new(
        agent_id: AgentID,
        executable_data: &ExecutableData,
        maybe_log_dir: Option<PathBuf>,
        group: G,
    ) -> Self {
        let mut cmd = Command::new(&executable_data.bin);
        cmd.args(&executable_data.args).envs(&executable_data.env);
        // Start the child in its own process group (pgid = child pid) so SIGTERM can be
        // delivered to the entire group without affecting Agent Control's own group.
        #[cfg(target_family = "unix")]
        cmd.process_group(0);

        Self {
            agent_id,
            cmd,
            maybe_log_dir,
            #[cfg(target_family = "unix")]
            shutdown_timeout: executable_data.shutdown_timeout,
            group,
        }
    }
}

impl<G: TrackedProcessGroup> OsProcessConfig<G> {
    /// Spawns the process with its stdout/stderr set up.
    pub fn start(mut self) -> Result<OsProcess<G>, CommandError> {
        let (stdout, stderr) = std_streams(&self.maybe_log_dir)?;
        self.cmd.stdout(stdout).stderr(stderr);
        let child = self.group.spawn(self.cmd).map_err(CommandError::Tracker)?;
        Ok(OsProcess {
            agent_id: self.agent_id,
            process: child,
            #[cfg(target_family = "unix")]
            shutdown_timeout: self.shutdown_timeout,
            group: self.group,
        })
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started OS Process
////////////////////////////////////////////////////////////////////////////////////

impl<G> OsProcess<G> {
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

    /// Hands over the group the process was started in.
    pub(crate) fn into_group(self) -> G {
        self.group
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::defaults::STDOUT_LOG_FILE_NAME;
    use crate::sub_agent::on_host::managed_process::spawned_process::restart_policy::RestartPolicy;
    use crate::sub_agent::on_host::process_group_tracker::tests::test_process_config;
    use crate::utils::retry::retry;
    use std::collections::HashMap;
    use std::time::Duration;

    /// Prints `late-output` to stdout after a short delay.
    fn delayed_output_command() -> (String, Vec<String>) {
        #[cfg(target_family = "unix")]
        let (bin, args) = ("sh", ["-c", "sleep 1; echo late-output"]);
        #[cfg(target_family = "windows")]
        let (bin, args) = (
            "powershell",
            [
                "-NoProfile",
                "-Command",
                "Start-Sleep -Seconds 1; Write-Output late-output",
            ],
        );
        (bin.to_string(), args.map(String::from).to_vec())
    }

    #[test]
    fn process_appends_to_log_file_after_being_dropped() {
        let log_dir = tempfile::tempdir().unwrap();
        let stdout_log = log_dir.path().join(STDOUT_LOG_FILE_NAME);
        std::fs::write(&stdout_log, "previous-output\n").unwrap();
        let (bin, args) = delayed_output_command();
        let exec_data = ExecutableData {
            id: "late-writer".to_string(),
            bin,
            args,
            env: HashMap::new(),
            restart_policy: RestartPolicy::default(),
            shutdown_timeout: Default::default(),
        };

        let process = test_process_config(
            AgentID::AgentControl,
            &exec_data,
            Some(log_dir.path().to_path_buf()),
        )
        .start()
        .unwrap();
        // Agent Control no longer holds any handle to the process or its log files, only to its
        // group, as a supervised process does.
        let _group = process.into_group();

        retry(10, Duration::from_millis(500), || {
            let content = std::fs::read_to_string(&stdout_log).map_err(|e| e.to_string())?;
            // The pre-existing content must be preserved, with the new output appended after it.
            (content.starts_with("previous-output\n") && content.contains("late-output"))
                .then_some(())
                .ok_or_else(|| format!("process output was not appended, got: {content:?}"))
        })
        .unwrap();
    }
}
