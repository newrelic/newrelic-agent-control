use super::error::CommandError;

/// ProcessTerminator it's a service that allows shutting down gracefully the process
/// with the pid provided or force killing it if the timeout provided is reached
pub struct ProcessTerminator {
    pid: u32,
}

impl ProcessTerminator {
    /// new creates a new ProcessTerminator for the pid provided
    pub fn new(pid: u32) -> Self {
        Self { pid }
    }

    #[cfg(target_family = "unix")]
    /// shutdown will attempt to kill a process with a SIGTERM if it succeeds the function F is
    /// executed to wait for the process to exit on time or the process is killed with a SIGKILL
    pub fn shutdown<F>(self, func: F) -> Result<(), CommandError>
    where
        F: FnOnce() -> bool,
    {
        use nix::{sys::signal, unistd::Pid};
        signal::kill(Pid::from_raw(self.pid as i32), signal::SIGTERM)
            .map_err(|err| CommandError::NixError(err.to_string()))?;

        if !func() {
            signal::kill(Pid::from_raw(self.pid as i32), signal::SIGKILL)
                .map_err(|err| CommandError::NixError(err.to_string()))?;
        }
        Ok(())
    }

    #[cfg(target_family = "windows")]
    pub fn shutdown<F>(self, _func: F) -> Result<(), CommandError>
    where
        F: FnOnce() -> bool,
    {
        unimplemented!("windows processes can't be shutdown")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use std::{
        process::Command,
        thread::{self, sleep},
        time::Duration,
    };

    #[rstest]
    #[case::custom_timeout(35, || false, "signal: 9 (SIGKILL)")]
    #[case::on_time(1, || true, "exit status: 0")]
    fn shutdown_custom_timeout(
        #[case] trap_sleep: u64,
        #[case] shutdown_fn: fn() -> bool,
        #[case] output: &str,
    ) {
        let mut trap_cmd = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "trap \"sleep {};exit 0\" TERM;while true; do sleep 1; done",
                trap_sleep
            ))
            .spawn();

        // Warm-up time for the trap sub-process to start and be able to catch the signal
        sleep(Duration::from_secs(1));

        let pid = trap_cmd.as_mut().unwrap().id();
        thread::spawn(move || {
            _ = ProcessTerminator::new(pid).shutdown(shutdown_fn);
        });

        // Wait for process to exit
        let result = trap_cmd.unwrap().wait();
        assert_eq!(output, result.unwrap().to_string());
    }
}
