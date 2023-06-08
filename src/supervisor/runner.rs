use std::{
    ffi::OsStr,
    ops::Deref,
    path::Path,
    sync::mpsc::Sender,
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
};
use std::time::Duration;

use crate::command::{
    stream::{Event, Metadata},
    wait_exit_timeout_default, CommandExecutor, CommandHandle, CommandTerminator, EventStreamer,
    ProcessRunner, ProcessTerminator,
};

use super::{
    backoff::{BackoffStrategy, Backoff},
    context::SupervisorContext,
    error::ProcessError,
    Handle,
    Runner,
    ID,
};

use log::error;

#[derive(Clone)]
pub struct Stopped {
    bin: String,
    args: Vec<String>,
    ctx: SupervisorContext,
    snd: Sender<Event>,
    backoff: BackoffStrategy,
}

pub struct Running {
    handle: JoinHandle<()>,
    ctx: SupervisorContext,
}

#[derive(Debug, Clone)]
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
    let mut retry_policy = runner.backoff.clone();
    thread::spawn({
        move || loop {
            let proc_runner = ProcessRunner::from(&runner).with_metadata(Metadata::from(&runner));

            // Actually run the process
            let started = match proc_runner.start() {
                Ok(s) => s,
                Err(e) => {
                    error!(supervisor = runner.id(); "Failed to start a supervised process: {}", e);
                    continue;
                }
            };

            // Stream the output
            let streaming = match started.stream(runner.snd.clone()) {
                Ok(s) => s,
                Err(e) => {
                    error!(supervisor = runner.id(); "Failed to stream the output of a supervised process: {}", e);
                    continue;
                }
            };

            _ = wait_for_termination(streaming.get_pid(), runner.ctx.clone());
            _ = streaming.wait().unwrap();

            let (lck, _) = SupervisorContext::get_lock_cvar(&runner.ctx);
            let val = lck.lock().unwrap();
            if *val {
                break;
            }

            if !retry_policy.backoff() {
                break;
            }
        }
    })
}

/// Blocks on the [`SupervisorContext`], [`ctx`]. When the termination signal is activated, this will send a shutdown signal to the process being supervised (the one whose PID was passed as [`pid`]).
fn wait_for_termination(pid: u32, ctx: SupervisorContext) -> JoinHandle<()> {
    thread::spawn(move || {
        let (lck, cvar) = SupervisorContext::get_lock_cvar(&ctx);
        _ = cvar.wait_while(lck.lock().unwrap(), |finish| !*finish);

        thread::spawn(move || {
            let shutdown_ctx = Arc::new((Mutex::new(false), Condvar::new()));
            _ = ProcessTerminator::new(pid).shutdown(|| wait_exit_timeout_default(shutdown_ctx));
        });
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
}

impl SupervisorRunner<Stopped> {
    pub fn new(
        bin: String,
        args: Vec<String>,
        ctx: SupervisorContext,
        snd: Sender<Event>,
        backoff: BackoffStrategy,
    ) -> Self {
        SupervisorRunner {
            state: Stopped {
                bin,
                args,
                ctx,
                snd,
                backoff,
            },
        }
    }

    pub fn with_restart_policy(&mut self, backoff_strategy: String, delay: Duration, max_retries: usize) -> Self {
        match backoff_strategy.as_str() {
            "fixed" => self.state.backoff = BackoffStrategy::Fixed(Backoff::new().with_initial_delay(delay).with_max_retries(max_retries)),
            "linear" => self.state.backoff = BackoffStrategy::Linear(Backoff::new().with_initial_delay(delay).with_max_retries(max_retries)),
            "exponential" => self.state.backoff = BackoffStrategy::Exponential(Backoff::new().with_initial_delay(delay).with_max_retries(max_retries)),
            unsupported => {
                error!("backoff type {} not supported", unsupported);
            }
        }
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::context;
    use crate::supervisor::backoff;
    use std::time::Duration;

    #[test]
    fn test_supervisor_fixed_retry_3_times() {
        let (tx, rx) = std::sync::mpsc::channel();
        let agent: SupervisorRunner = SupervisorRunner::new(
            "echo".to_owned(),
            vec!["hello!".to_owned()],
            SupervisorContext::new(),
            tx.clone(),
            BackoffStrategy::None,
        ).with_restart_policy("linear".to_string(), Duration::new(0, 100), 3);

        let agent = agent.run();

        let stream = thread::spawn(move || {
            let mut stdout_actual = Vec::new();

            loop {
                match rx.recv() {
                    Err(_) => break,
                    Ok(event) => match event {
                        OutputEvent::Stdout(line) => {
                            stdout_actual.push(line)
                        },
                        OutputEvent::Stderr(line) => (),
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

        assert_eq!(4, stdout.iter().count());
    }
}
