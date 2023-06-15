use std::{
    ffi::OsStr,
    ops::Deref,
    path::Path,
    sync::mpsc::Sender,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

use crate::{
    command::{
        stream::{Event, Metadata},
        wait_exit_timeout_default, CommandExecutor, CommandHandle, CommandTerminator,
        EventStreamer, ProcessRunner, ProcessTerminator,
    },
    context::Context,
};

use super::{
    error::ProcessError,
    restart::{BackoffStrategy, RestartPolicy},
    Handle, Runner, ID,
};

use tracing::{error, info};

pub struct Stopped {
    bin: String,
    args: Vec<String>,
    ctx: Context,
    snd: Sender<Event>,
    restart: RestartPolicy,
}

pub struct Running {
    handle: JoinHandle<()>,
    ctx: Context,
}

#[derive(Debug)]
pub struct SupervisorRunner<State = Stopped> {
    state: State,
}

impl<T> Deref for SupervisorRunner<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

// TODO: change with agent identifier (infra_agent/gateway)
impl From<&SupervisorRunner<Stopped>> for String {
    fn from(value: &SupervisorRunner<Stopped>) -> Self {
        value.bin.clone()
    }
}

impl ID for SupervisorRunner<Stopped> {
    fn id(&self) -> String {
        String::from(self)
    }
}

impl Runner for SupervisorRunner<Stopped> {
    type E = ProcessError;
    type H = SupervisorRunner<Running>;

    fn run(self) -> Self::H {
        let ctx = self.ctx.clone();
        SupervisorRunner {
            state: Running {
                handle: run_process_thread(self),
                ctx,
            },
        }
    }
}

impl From<&SupervisorRunner<Stopped>> for ProcessRunner {
    fn from(value: &SupervisorRunner<Stopped>) -> Self {
        ProcessRunner::new(&value.bin, &value.args)
    }
}

impl From<&SupervisorRunner<Stopped>> for Metadata {
    // use binary file name as supervisor id
    fn from(value: &SupervisorRunner<Stopped>) -> Self {
        Metadata::new(
            Path::new(&value.bin)
                .file_name()
                .unwrap_or(OsStr::new("not found"))
                .to_string_lossy(),
        )
    }
}

fn run_process_thread(runner: SupervisorRunner<Stopped>) -> JoinHandle<()> {
    let mut restart_policy = runner.restart.clone();
    let mut code = 0;
    let current_pid: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));

    let shutdown_ctx = Context::new();
    _ = wait_for_termination(
        current_pid.clone(),
        runner.ctx.clone(),
        shutdown_ctx.clone(),
    );
    thread::spawn({
        move || loop {
            let proc_runner = ProcessRunner::from(&runner).with_metadata(Metadata::from(&runner));

            let (lck, _) = Context::get_lock_cvar(&runner.ctx);
            let val = lck.lock().unwrap();
            if *val {
                break;
            }

            if !restart_policy.should_retry(code) {
                break;
            }
            restart_policy.backoff();

            info!(
                supervisor = runner.id(),
                msg = "Starting supervisor process"
            );

            // Actually run the process
            let Ok(started) = proc_runner.start().map_err(|e| {
                error!(
                    supervisor = runner.id(),
                    "Failed to start a supervised process: {}", e
                );
            }) else { continue };

            // Stream the output
            let Ok(streaming) = started.stream(runner.snd.clone()).map_err(|e| {
                error!(
                    supervisor = runner.id(),
                    "Failed to stream the output of a supervised process: {}", e
                );
            }) else { continue };
            *current_pid.lock().unwrap() = streaming.get_pid();
            shutdown_ctx.reset().unwrap();

            // Signals return exit_code 0, if in the future we need to act on them we can import
            // std::os::unix::process::ExitStatusExt to get the code with the method into_raw
            let exit_code = streaming.wait().unwrap().code();
            if let Some(c) = exit_code {
                code = c
            }
            *current_pid.lock().unwrap() = None;
            shutdown_ctx.cancel_all().unwrap();
        }
    })
}

/// Blocks on the [`Context`], [`ctx`]. When the termination signal is activated, this will send a shutdown signal to the process being supervised (the one whose PID was passed as [`pid`]).
fn wait_for_termination(
    current_pid: Arc<Mutex<Option<u32>>>,
    ctx: Context,
    shutdown_ctx: Context,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let (lck, cvar) = Context::get_lock_cvar(&ctx);
        _ = cvar.wait_while(lck.lock().unwrap(), |finish| !*finish);

        if let Some(pid) = *current_pid.lock().unwrap() {
            _ = ProcessTerminator::new(pid).shutdown(|| wait_exit_timeout_default(shutdown_ctx));
        }
    })
}

impl Handle for SupervisorRunner<Running> {
    type E = ProcessError;
    type S = JoinHandle<()>;

    fn stop(self) -> Self::S {
        // Stop all the supervisors
        // TODO: handle PoisonErrors (log?)
        self.ctx.cancel_all().unwrap();
        self.state.handle
    }

    fn wait(self) -> Result<(), Self::E> {
        self.state
            .handle
            .join()
            .map_err(|_| ProcessError::ThreadError)
    }

    fn is_finished(&self) -> bool {
        self.state.handle.is_finished()
    }
}

impl SupervisorRunner<Stopped> {
    pub fn new(bin: String, args: Vec<String>, ctx: Context, snd: Sender<Event>) -> Self {
        SupervisorRunner {
            state: Stopped {
                bin,
                args,
                ctx,
                snd,
                restart: RestartPolicy::new(BackoffStrategy::None, Vec::new()),
            },
        }
    }

    pub fn with_restart_policy(
        mut self,
        restart_exit_codes: Vec<i32>,
        backoff_strategy: BackoffStrategy,
    ) -> Self {
        self.state.restart = RestartPolicy::new(backoff_strategy, restart_exit_codes);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::stream::OutputEvent;
    use crate::supervisor::restart::Backoff;
    use std::time::Duration;

    #[test]
    fn test_supervisor_retries_and_exits_on_wrong_command() {
        let (tx, _) = std::sync::mpsc::channel();

        let backoff = Backoff::new()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let agent: SupervisorRunner = SupervisorRunner::new(
            "wrong-command".to_owned(),
            vec!["x".to_owned()],
            Context::new(),
            tx.clone(),
        )
        .with_restart_policy(vec![0], BackoffStrategy::Fixed(backoff));

        let agent = agent.run();

        while !agent.handle.is_finished() {
            thread::sleep(Duration::from_millis(15));
        }

        drop(tx);
    }

    #[test]
    fn test_supervisor_fixed_retry_3_times() {
        let (tx, rx) = std::sync::mpsc::channel();

        let backoff = Backoff::new()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let agent: SupervisorRunner = SupervisorRunner::new(
            "echo".to_owned(),
            vec!["hello!".to_owned()],
            Context::new(),
            tx.clone(),
        )
        .with_restart_policy(vec![0], BackoffStrategy::Fixed(backoff));

        let agent = agent.run();

        let stream = thread::spawn(move || {
            let mut stdout_actual = Vec::new();

            loop {
                match rx.recv() {
                    Err(_) => break,
                    Ok(event) => match event.output {
                        OutputEvent::Stdout(line) => stdout_actual.push(line),
                        OutputEvent::Stderr(_) => (),
                    },
                }
            }

            stdout_actual
        });

        while !agent.handle.is_finished() {
            thread::sleep(Duration::from_millis(15));
        }

        drop(tx);
        let stdout = stream.join().unwrap();

        assert_eq!(3, stdout.iter().count());
    }
}
