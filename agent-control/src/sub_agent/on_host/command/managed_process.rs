//! Abstraction over an OS process supervised by Agent Control, either spawned by it or adopted
//! from a previous instance.
//!
//! `std::process::Child` can only be built through `Command::spawn`, so it cannot represent a
//! process that was already running when Agent Control started.

use std::io;
use std::process::{Child, ChildStderr, ChildStdout, ExitStatus};

/// A process supervised by Agent Control.
pub enum ManagedProcess {
    /// Spawned by this Agent Control instance, which is therefore its real parent.
    Spawned(Child),
    /// Already running when this instance started. Agent Control is not its parent.
    // Nothing constructs it until process adoption is implemented.
    #[allow(dead_code)]
    Adopted(AdoptedProcess),
}

/// A running process Agent Control did not spawn.
// Placeholder: the behavior is pending the adoption design.
#[allow(dead_code)]
pub struct AdoptedProcess {
    pid: u32,
    /// Platform-dependent data to recognize the exact process instance.
    additional_data: u64,
}

impl ManagedProcess {
    /// OS process id.
    pub fn id(&self) -> u32 {
        match self {
            Self::Spawned(child) => child.id(),
            Self::Adopted(adopted) => adopted.pid,
        }
    }

    /// Returns whether the process is still running.
    pub fn is_alive(&mut self) -> bool {
        match self {
            Self::Spawned(child) => child.try_wait().is_ok_and(|status| status.is_none()),
            Self::Adopted(_) => unimplemented!("adopted processes are not supported yet"),
        }
    }

    /// Forcefully terminates the process.
    pub fn kill(&mut self) -> io::Result<()> {
        match self {
            Self::Spawned(child) => child.kill(),
            Self::Adopted(_) => unimplemented!("adopted processes are not supported yet"),
        }
    }

    /// Blocks until the process exits.
    pub fn wait_for_exit(&mut self) -> io::Result<ExitStatus> {
        match self {
            Self::Spawned(child) => child.wait(),
            Self::Adopted(_) => unimplemented!("adopted processes are not supported yet"),
        }
    }

    /// Takes the piped stdout, if any.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        match self {
            Self::Spawned(child) => child.stdout.take(),
            Self::Adopted(_) => unimplemented!("adopted processes are not supported yet"),
        }
    }

    /// Takes the piped stderr, if any.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        match self {
            Self::Spawned(child) => child.stderr.take(),
            Self::Adopted(_) => unimplemented!("adopted processes are not supported yet"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[cfg(target_family = "unix")]
    fn long_running_command() -> Command {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        cmd
    }

    #[cfg(target_family = "windows")]
    fn long_running_command() -> Command {
        let mut cmd = Command::new("ping");
        cmd.args(["-n", "30", "127.0.0.1"]);
        cmd
    }

    #[cfg(target_family = "unix")]
    fn short_lived_command() -> Command {
        Command::new("true")
    }

    #[cfg(target_family = "windows")]
    fn short_lived_command() -> Command {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "exit", "0"]);
        cmd
    }

    #[test]
    fn spawned_reports_child_pid() {
        let child = long_running_command().spawn().unwrap();
        let pid = child.id();
        let mut process = ManagedProcess::Spawned(child);

        assert_eq!(process.id(), pid);

        process.kill().unwrap();
        process.wait_for_exit().unwrap();
    }

    #[test]
    fn spawned_is_alive_until_killed() {
        let mut process = ManagedProcess::Spawned(long_running_command().spawn().unwrap());
        assert!(process.is_alive());

        process.kill().unwrap();
        process.wait_for_exit().unwrap();

        assert!(!process.is_alive());
    }

    #[test]
    fn spawned_wait_for_exit_returns_exit_status() {
        let mut process = ManagedProcess::Spawned(short_lived_command().spawn().unwrap());

        let status = process.wait_for_exit().unwrap();

        assert!(status.success());
        assert!(!process.is_alive());
    }

    #[test]
    fn spawned_killed_process_reports_unsuccessful_status() {
        let mut process = ManagedProcess::Spawned(long_running_command().spawn().unwrap());

        process.kill().unwrap();

        assert!(!process.wait_for_exit().unwrap().success());
    }

    #[test]
    fn spawned_exposes_piped_output_once() {
        let mut process = ManagedProcess::Spawned(
            short_lived_command()
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );

        assert!(process.take_stdout().is_some());
        assert!(process.take_stdout().is_none());
        assert!(process.take_stderr().is_some());
        assert!(process.take_stderr().is_none());

        process.wait_for_exit().unwrap();
    }
}
