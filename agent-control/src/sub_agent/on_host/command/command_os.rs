use tracing::warn;

use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{STDERR_LOG_PREFIX, STDOUT_LOG_PREFIX};
use crate::sub_agent::on_host::command::executable_data::ExecutableData;
use std::time::{Duration, Instant};
use std::{
    io,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
};

use super::{
    error::CommandError,
    logging::{
        self,
        file_logger::{FileAppender, FileLogger, FileSystemLoggers},
        logger::Logger,
    },
};

const POLL_INTERVAL: Duration = Duration::from_millis(100);

////////////////////////////////////////////////////////////////////////////////////
// States for Started/Not Started/Sync Command
////////////////////////////////////////////////////////////////////////////////////
pub struct CommandOSNotStarted {
    cmd: Command,
    agent_id: AgentID,
    logs_to_file: bool,
    logging_path: PathBuf,
    shutdown_timeout: Duration,
}
pub struct CommandOSStarted {
    agent_id: AgentID,
    process: Child,
    loggers: Option<FileSystemLoggers>,
    shutdown_timeout: Duration,
}

////////////////////////////////////////////////////////////////////////////////////
// Not Started Command OS
////////////////////////////////////////////////////////////////////////////////////
impl CommandOSNotStarted {
    pub fn new(
        agent_id: AgentID,
        executable_data: &ExecutableData,
        logs_to_file: bool,
        logging_path: PathBuf,
    ) -> Self {
        let mut cmd = Command::new(&executable_data.bin);
        cmd.args(&executable_data.args)
            .envs(&executable_data.env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(target_family = "windows")]
        Self::create_process_group(&mut cmd);

        Self {
            agent_id,
            cmd,
            logs_to_file,
            logging_path,
            shutdown_timeout: executable_data.shutdown_timeout,
        }
    }

    pub fn start(mut self) -> Result<CommandOSStarted, CommandError> {
        let agent_id = self.agent_id;
        let loggers = self.logs_to_file.then(|| {
            FileSystemLoggers::new(
                file_logger(&agent_id, self.logging_path.clone(), STDOUT_LOG_PREFIX),
                file_logger(&agent_id, self.logging_path, STDERR_LOG_PREFIX),
            )
        });
        Ok(CommandOSStarted {
            agent_id,
            process: self.cmd.spawn()?,
            loggers,
            shutdown_timeout: self.shutdown_timeout,
        })
    }

    #[cfg(target_family = "windows")]
    /// Sets the process creation flags to create a new process group for Windows processes.
    ///
    /// This enables sending CTRL+BREAK events to it via the [`GenerateConsoleCtrlEvent`](windows::Win32::System::Console::GenerateConsoleCtrlEvent) function,
    /// which is the mechanism we use to gracefully shut down the process. Otherwise, the Agent Control process needs to attach itself to the
    /// console of the process to send a CTRL+C event which would need synchronization (many sub-agents making AC attach and reattach concurrently).
    ///
    /// For details, see the [task termination mechanism for GitLab runners](https://gitlab.com/gitlab-org/gitlab-runner/-/blob/397ba5dc2685e7b13feaccbfed4c242646955334/helpers/process/killer_windows.go#L75-108), which can use either mechanism dependent on a flag to use the legacy method (attach and reattach the parent process).
    ///
    /// Additional reading:
    ///   - [`GenerateConsoleCtrlEvent` function](https://learn.microsoft.com/en-us/windows/console/generateconsolectrlevent), see second parameter `dwProcessGroupId`.
    ///   - [Process Creation Flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags)
    fn create_process_group(cmd: &mut Command) {
        use std::os::windows::process::CommandExt;
        use windows::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;

        // Create new process group so we can send CTRL+BREAK events to it
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP.0);
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started Command OS
////////////////////////////////////////////////////////////////////////////////////

impl CommandOSStarted {
    pub fn get_pid(&self) -> u32 {
        self.process.id()
    }

    pub fn is_running(&mut self) -> bool {
        self.process.try_wait().is_ok_and(|v| v.is_none())
    }

    pub(crate) fn wait(mut self) -> Result<ExitStatus, CommandError> {
        self.process.wait().map_err(CommandError::from)
    }

    pub(crate) fn stream(mut self) -> Result<Self, CommandError> {
        let stdout = self
            .process
            .stdout
            .take()
            .ok_or(CommandError::StreamPipeError("stdout".to_string()))?;

        let stderr = self
            .process
            .stderr
            .take()
            .ok_or(CommandError::StreamPipeError("stderr".to_string()))?;

        let mut stdout_loggers = vec![Logger::Stdout(self.agent_id.clone())];
        let mut stderr_loggers = vec![Logger::Stderr(self.agent_id.clone())];

        if let Some(l) = self.loggers.take() {
            let (out, err) = l.into_loggers();
            stdout_loggers.push(Logger::File(Box::new(out), self.agent_id.clone()));
            stderr_loggers.push(Logger::File(Box::new(err), self.agent_id.clone()));
        };

        // Read stdout and send to the channel
        logging::thread::spawn_logger(stdout, stdout_loggers);

        // Read stderr and send to the channel
        logging::thread::spawn_logger(stderr, stderr_loggers);

        Ok(self)
    }

    fn is_running_after_timeout(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;

        while Instant::now() < deadline {
            if !self.is_running() {
                return false;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        true
    }

    pub fn shutdown(&mut self) -> Result<(), CommandError> {
        let pid = self.get_pid();
        // Attempt a graceful shutdown (platform-dependent).
        let graceful_shutdown_result = Self::graceful_shutdown(pid);

        if let Err(e) = &graceful_shutdown_result {
            warn!(agent_id = %self.agent_id, "Graceful shutdown failed for process {pid}: {e}");
        }

        if graceful_shutdown_result.is_err() || self.is_running_after_timeout(self.shutdown_timeout)
        {
            self.process.kill().map_err(CommandError::from)?;
        }

        Ok(())
    }

    #[cfg(target_family = "unix")]
    fn graceful_shutdown(pid: u32) -> Result<(), CommandError> {
        use nix::{sys::signal, unistd::Pid};

        signal::kill(Pid::from_raw(pid as i32), signal::SIGTERM)
            .map_err(|e| CommandError::from(io::Error::from(e)))
    }

    #[cfg(target_family = "windows")]
    fn graceful_shutdown(pid: u32) -> Result<(), CommandError> {
        use windows::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
        // Graceful shutdown for console applications
        // <https://stackoverflow.com/a/12899284>
        // <https://gitlab.com/gitlab-org/gitlab-runner/-/blob/397ba5dc2685e7b13feaccbfed4c242646955334/helpers/process/killer_windows.go#L75-108>
        unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) }
            .map_err(|e| CommandError::from(io::Error::from(e)))
    }
}

/// Creates a new file logger for this agent id and file prefix
fn file_logger(agent_id: &AgentID, path: PathBuf, prefix: &str) -> FileLogger {
    FileAppender::new(agent_id, path, prefix).into()
}
