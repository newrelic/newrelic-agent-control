use super::command::CommandError;
use crate::context::Context;
#[cfg(target_family = "unix")]
use nix::{sys::signal, unistd::Pid};
use std::time::Duration;
use tracing::error;

/// DEFAULT_EXIT_TIMEOUT of 2 seconds
const DEFAULT_EXIT_TIMEOUT: Duration = Duration::new(10, 0);

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
        signal::kill(Pid::from_raw(self.pid as i32), signal::SIGTERM)?;
        if !func() {
            signal::kill(Pid::from_raw(self.pid as i32), signal::SIGKILL)?;
        }
        Ok(())
    }

    #[cfg(not(target_family = "unix"))]
    fn shutdown<F>(self, func: F) -> Result<(), Self::Error>
    where
        F: FnOnce() -> bool,
    {
        unimplemented!("windows processes can't be shutdown")
    }
}

/// wait_exit_timeout is a function that waits on a condvar for a change in a boolean exit variable
/// but returning a false if the timeout provided is reached before any state change.
pub fn wait_exit_timeout(context: Context<bool>, exit_timeout: Duration) -> bool {
    let (lock, cvar) = context.get_lock_cvar();
    let exited = lock.lock();
    match exited {
        Ok(mut exited) => loop {
            let result = cvar.wait_timeout(exited, exit_timeout);
            match result {
                Ok(result) => {
                    exited = result.0;
                    let timer = result.1;
                    if timer.timed_out() {
                        return false;
                    }
                    if *exited {
                        return true;
                    }
                }
                Err(error) => {
                    error!("wait error: {}", error);
                    return false;
                }
            }
        },
        Err(error) => {
            error!("lock error: {}", error);
            false
        }
    }
}

/// waits on a condvar for a change in a boolean exit variable
/// with a default timeout of DEFAULT_EXIT_TIMEOUT seconds
pub fn wait_exit_timeout_default(context: Context<bool>) -> bool {
    wait_exit_timeout(context, DEFAULT_EXIT_TIMEOUT)
}

#[cfg(target_family = "unix")]
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::Command,
        thread::{self, sleep},
    };

    #[test]
    fn shutdown_custom_timeout() {
        let mut trap_cmd = Command::new("sh")
            .arg("-c")
            .arg("trap \"sleep 35;exit 0\" TERM;while true; do sleep 1; done")
            .spawn();

        let pid = trap_cmd.as_mut().unwrap().id();

        // Warm-up time for the trap sub-process to start and be able to catch the signal
        let one_second = Duration::from_secs(1);
        sleep(one_second);

        let terminator = ProcessTerminator::new(pid);

        let context = Context::new();
        let context_child = context.clone();

        thread::spawn(|| {
            _ = terminator
                .shutdown(|| wait_exit_timeout(context_child, Duration::from_millis(300)));
        });

        // Wait for process to exit
        let result = trap_cmd.unwrap().wait();

        // We update the status o cvar to notify it exited
        let (lock, cvar) = context.get_lock_cvar();
        let mut exited = lock.lock().unwrap();
        *exited = true;
        cvar.notify_all();

        assert_eq!("signal: 9 (SIGKILL)", result.unwrap().to_string());
    }

    #[test]
    fn shutdown_on_time() {
        let mut trap_cmd = Command::new("sh")
            .arg("-c")
            .arg("trap \"sleep 1;exit 0\" TERM;while true; do sleep 1; done")
            .spawn();

        let pid = trap_cmd.as_mut().unwrap().id();
        let one_second = Duration::from_secs(1);
        sleep(one_second);

        let terminator = ProcessTerminator::new(pid);

        let context = Context::new();
        let context_child = context.clone();

        thread::spawn(|| {
            _ = terminator.shutdown(|| wait_exit_timeout(context_child, Duration::new(3, 0)));
        });

        // Wait for process to exit
        let result = trap_cmd.unwrap().wait();

        // We update the status o cvar to notify it exited
        let (lock, cvar) = context.get_lock_cvar();
        let mut exited = lock.lock().unwrap();
        *exited = true;
        cvar.notify_all();

        assert_eq!("exit status: 0", result.unwrap().to_string());
    }
}
