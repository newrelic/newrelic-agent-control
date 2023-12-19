use std::ffi::OsStr;
use std::path::Path;
use std::process::ExitStatus;
use std::{
    ops::Deref,
    sync::mpsc::Sender,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

use crate::context::Context;
use crate::sub_agent::logger::{AgentLog, Metadata};

use crate::sub_agent::on_host::command::command::{
    CommandError, CommandTerminator, NotStartedCommand, StartedCommand,
};
use crate::sub_agent::on_host::command::command_os;
use crate::sub_agent::on_host::command::command_os::CommandOS;
use crate::sub_agent::on_host::command::shutdown::{
    wait_exit_timeout, wait_exit_timeout_default, ProcessTerminator,
};
use crate::sub_agent::on_host::supervisor::command_supervisor_config::SupervisorConfigOnHost;
use crate::sub_agent::on_host::supervisor::error::SupervisorError;
use tracing::{error, info};

////////////////////////////////////////////////////////////////////////////////////
// States for Started/Not Started supervisor
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStarted {
    config: SupervisorConfigOnHost,
}
pub struct Started {
    handle: JoinHandle<()>,
    ctx: Context<bool>,
}

#[derive(Debug)]
pub struct SupervisorOnHost<S> {
    state: S,
}

impl SupervisorOnHost<NotStarted> {
    pub fn new(config: SupervisorConfigOnHost) -> Self {
        SupervisorOnHost {
            state: NotStarted { config },
        }
    }

    pub fn id(&self) -> String {
        self.state.config.bin.clone()
    }

    pub fn run(self) -> Result<SupervisorOnHost<Started>, SupervisorError> {
        //TODO: validate binary path, exec permissions...?
        let ctx = self.state.config.ctx.clone();
        Ok(SupervisorOnHost {
            state: Started {
                handle: start_process_thread(self),
                ctx,
            },
        })
    }

    pub fn not_started_command(&self) -> CommandOS<command_os::NotStarted> {
        //TODO extract to to a builder so we can mock it
        CommandOS::<command_os::NotStarted>::new(
            &self.state.config.bin,
            &self.state.config.args,
            &self.state.config.env,
        )
    }

    pub fn metadata(&self) -> Metadata {
        Metadata::new(
            Path::new(&self.state.config.bin)
                .file_name()
                .unwrap_or(OsStr::new("not found"))
                .to_string_lossy(),
        )
    }
}

impl Deref for SupervisorOnHost<NotStarted> {
    type Target = SupervisorConfigOnHost;
    fn deref(&self) -> &Self::Target {
        &self.state.config
    }
}

impl SupervisorOnHost<Started> {
    pub fn stop(self) -> JoinHandle<()> {
        // Stop all the supervisors
        // TODO: handle PoisonErrors (log?)
        self.state.ctx.cancel_all(true).unwrap();
        self.state.handle
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Helpers (TODO: Review and move?)
////////////////////////////////////////////////////////////////////////////////////

// launch_process starts a new process with a streamed channel and sets its current pid
// into the provided variable. It waits until the process exits.
fn start_command<R>(
    not_started_command: R,
    pid: Arc<Mutex<Option<u32>>>,
    tx: Sender<AgentLog>,
) -> Result<ExitStatus, CommandError>
where
    R: NotStartedCommand,
{
    // run and stream the process
    let streaming = not_started_command.start()?.stream(tx)?;

    // set current running pid
    *pid.lock().unwrap() = Some(streaming.get_pid());

    streaming.wait()
}

fn start_process_thread(not_started_supervisor: SupervisorOnHost<NotStarted>) -> JoinHandle<()> {
    let mut restart_policy = not_started_supervisor.restart_policy.clone();
    let current_pid: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));

    let shutdown_ctx = Context::new();
    _ = wait_for_termination(
        current_pid.clone(),
        not_started_supervisor.ctx.clone(),
        shutdown_ctx.clone(),
    );
    thread::spawn({
        move || loop {
            // check if supervisor context is cancelled
            if *Context::get_lock_cvar(&not_started_supervisor.ctx)
                .0
                .lock()
                .unwrap()
            {
                break;
            }

            info!(
                supervisor = not_started_supervisor.id(),
                msg = "Starting supervisor process"
            );

            shutdown_ctx.reset().unwrap();
            // Signals return exit_code 0, if in the future we need to act on them we can import
            // std::os::unix::process::ExitStatusExt to get the code with the method into_raw
            let not_started_command = not_started_supervisor.not_started_command();
            let id = not_started_supervisor.id();
            let exit_code = start_command(
                not_started_command.with_metadata(not_started_supervisor.metadata()),
                current_pid.clone(),
                not_started_supervisor.snd.clone(),
            )
            .map_err(|err| {
                error!(
                    supervisor = id,
                    "Error while launching supervisor process: {}", err
                );
            })
            .map(|exit_code| {
                if !exit_code.success() {
                    error!(
                        supervisor = id,
                        exit_code = exit_code.code(),
                        "Supervisor process exited unsuccessfully"
                    )
                }
                exit_code.code()
            });

            // canceling the shutdown ctx must be done before getting current_pid lock
            // as it locked by the wait_for_termination function
            shutdown_ctx.cancel_all(true).unwrap();
            *current_pid.lock().unwrap() = None;

            // check if restart policy needs to be applied
            if !restart_policy.should_retry(exit_code.unwrap_or_default()) {
                break;
            }

            restart_policy.backoff(|duration| {
                // early exit if supervisor timeout is canceled
                wait_exit_timeout(not_started_supervisor.ctx.clone(), duration);
            });
        }
    })
}

/// Blocks on the [`Context`], [`ctx`]. When the termination signal is activated, this will send a shutdown signal to the process being supervised (the one whose PID was passed as [`pid`]).
fn wait_for_termination(
    current_pid: Arc<Mutex<Option<u32>>>,
    ctx: Context<bool>,
    shutdown_ctx: Context<bool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let (lck, cvar) = Context::get_lock_cvar(&ctx);
        _ = cvar.wait_while(lck.lock().unwrap(), |finish| !*finish);

        if let Some(pid) = *current_pid.lock().unwrap() {
            _ = ProcessTerminator::new(pid).shutdown(|| wait_exit_timeout_default(shutdown_ctx));
        }
    })
}

#[cfg(test)]
pub mod sleep_supervisor_tests {
    use std::collections::HashMap;
    use std::sync::mpsc::Sender;

    use super::SupervisorOnHost;
    use super::{NotStarted, SupervisorConfigOnHost};
    use crate::sub_agent::restart_policy::{BackoffStrategy, RestartPolicy};
    use crate::{context::Context, sub_agent::logger::AgentLog};

    pub fn new_sleep_supervisor(
        tx: Sender<AgentLog>,
        seconds: u32,
    ) -> SupervisorOnHost<NotStarted> {
        let config = SupervisorConfigOnHost::new(
            "sh".to_owned(),
            vec!["-c".to_string(), format!("sleep {}", seconds)],
            Context::new(),
            HashMap::new(),
            tx.clone(),
            RestartPolicy::new(BackoffStrategy::None, Vec::new()),
        );
        SupervisorOnHost::new(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sub_agent::logger::LogOutput;
    use crate::sub_agent::restart_policy::{Backoff, BackoffStrategy, RestartPolicy};
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    #[test]
    fn test_supervisor_retries_and_exits_on_wrong_command() {
        let (tx, _) = std::sync::mpsc::channel();

        let backoff = Backoff::new()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let config = SupervisorConfigOnHost::new(
            "wrong-command".to_owned(),
            vec!["x".to_owned()],
            Context::new(),
            HashMap::new(),
            tx.clone(),
            RestartPolicy::new(BackoffStrategy::Fixed(backoff), vec![0]),
        );
        let agent = SupervisorOnHost::new(config);

        let agent = agent.run().unwrap();

        while !agent.state.handle.is_finished() {
            thread::sleep(Duration::from_millis(15));
        }
    }

    #[test]
    fn test_supervisor_restart_policy_early_exit() {
        let (tx, _) = std::sync::mpsc::channel();

        let timer = Instant::now();

        // set a fixed backoff of 10 seconds
        let backoff = Backoff::new()
            .with_initial_delay(Duration::from_secs(10))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let config = SupervisorConfigOnHost::new(
            "wrong-command".to_owned(),
            vec!["x".to_owned()],
            Context::new(),
            HashMap::new(),
            tx.clone(),
            RestartPolicy::new(BackoffStrategy::Fixed(backoff), vec![0]),
        );
        let agent = SupervisorOnHost::new(config);

        // run the agent with wrong command so it enters in restart policy
        let agent = agent.run().unwrap();
        // wait two seconds to ensure restart policy thread is sleeping
        thread::sleep(Duration::from_secs(2));
        assert!(agent.stop().join().is_ok());

        assert!(timer.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn test_supervisor_fixed_backoff_retry_3_times() {
        let (tx, rx) = std::sync::mpsc::channel();

        let backoff = Backoff::new()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let config = SupervisorConfigOnHost::new(
            "echo".to_owned(),
            vec!["hello!".to_owned()],
            Context::new(),
            HashMap::new(),
            tx,
            RestartPolicy::new(BackoffStrategy::Fixed(backoff), vec![0]),
        );
        let agent = SupervisorOnHost::new(config);

        let agent = agent.run().unwrap();

        let stream = thread::spawn(move || {
            let mut stdout_actual = Vec::new();

            loop {
                match rx.recv() {
                    Err(_) => break,
                    Ok(event) => match event.output {
                        LogOutput::Stdout(line) => stdout_actual.push(line),
                        LogOutput::Stderr(_) => (),
                    },
                }
            }

            stdout_actual
        });

        while !agent.state.handle.is_finished() {
            thread::sleep(Duration::from_millis(15));
        }

        let stdout = stream.join().unwrap();

        // 1 base execution + 3 retries
        assert_eq!(4, stdout.len());
    }
}
