use std::process::ExitStatus;
use std::{
    ops::Deref,
    sync::mpsc::Sender,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

use crate::command::error::CommandError;
use crate::command::processrunner::{ProcessRunnerBuilder, Unstarted};
use crate::command::CommandBuilder;
use crate::{
    command::{
        stream::{Event, Metadata},
        wait_exit_timeout, wait_exit_timeout_default, CommandExecutor, CommandHandle,
        CommandTerminator, EventStreamer, ProcessRunner, ProcessTerminator,
    },
    context::Context,
};

use super::{
    error::ProcessError,
    restart::{BackoffStrategy, RestartPolicy},
    Handle, Runner, ID,
};

use tracing::{error, info};

pub struct Stopped<B = ProcessRunnerBuilder>
where
    B: CommandBuilder,
{
    process_builder: B,
    ctx: Context<bool>,
    snd: Sender<Event>,
    restart: RestartPolicy,
}

pub struct Running {
    handle: JoinHandle<()>,
    ctx: Context<bool>,
}

#[derive(Debug)]
pub struct SupervisorRunner<State = Stopped> {
    state: State,
    // ID corresponds to the string serialization of AgentType
    id: String,
}

impl<T> Deref for SupervisorRunner<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl ID for SupervisorRunner<Stopped> {
    fn id(&self) -> String {
        self.id.clone()
    }
}

impl Runner for SupervisorRunner<Stopped> {
    type E = ProcessError;
    type H = SupervisorRunner<Running>;

    fn run(self) -> Self::H {
        let ctx = self.ctx.clone();
        let id = self.id.clone();
        SupervisorRunner {
            state: Running {
                handle: run_process_thread(self),
                ctx,
            },
            id,
        }
    }
}

impl From<&SupervisorRunner<Stopped>> for ProcessRunner {
    fn from(value: &SupervisorRunner<Stopped>) -> Self {
        value.process_builder.build()
    }
}

impl From<&SupervisorRunner<Stopped>> for Metadata {
    fn from(value: &SupervisorRunner<Stopped>) -> Self {
        Metadata::new(value.id())
    }
}

// launch_process starts a new process with a streamed channel and sets its current pid
// into the provided variable. It waits until the process exits.
fn launch_process(
    process: ProcessRunner<Unstarted>,
    pid: Arc<Mutex<Option<u32>>>,
    tx: Sender<Event>,
) -> Result<ExitStatus, CommandError> {
    // run and stream the process
    let streaming = process.start()?.stream(tx)?;

    // set current running pid
    *pid.lock().unwrap() = Some(streaming.get_pid());

    streaming.wait()
}

fn run_process_thread(runner: SupervisorRunner<Stopped>) -> JoinHandle<()> {
    let mut restart_policy = runner.restart.clone();
    let current_pid: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));

    let shutdown_ctx = Context::new();
    _ = wait_for_termination(
        current_pid.clone(),
        runner.ctx.clone(),
        shutdown_ctx.clone(),
    );
    thread::spawn({
        move || loop {
            // check if supervisor context is cancelled
            if *Context::get_lock_cvar(&runner.ctx).0.lock().unwrap() {
                break;
            }

            info!(
                supervisor = runner.id(),
                msg = "Starting supervisor process"
            );

            shutdown_ctx.reset().unwrap();
            // Signals return exit_code 0, if in the future we need to act on them we can import
            // std::os::unix::process::ExitStatusExt to get the code with the method into_raw
            let exit_code = launch_process(
                ProcessRunner::from(&runner).with_metadata(Metadata::from(&runner)),
                current_pid.clone(),
                runner.snd.clone(),
            )
            .map_err(|err| {
                error!(
                    supervisor = runner.id(),
                    "Error while launching supervisor process: {}", err
                );
            })
            .map(|exit_code| {
                if !exit_code.success() {
                    error!(
                        supervisor = runner.id(),
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
                wait_exit_timeout(runner.ctx.clone(), duration);
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

impl Handle for SupervisorRunner<Running> {
    type E = ProcessError;
    type S = JoinHandle<()>;

    fn stop(self) -> Self::S {
        // Stop all the supervisors
        // TODO: handle PoisonErrors (log?)
        self.ctx.cancel_all(true).unwrap();
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

impl Stopped {
    fn new<B>(process_builder: B, ctx: Context<bool>, snd: Sender<Event>) -> Stopped<B>
    where
        B: CommandBuilder,
    {
        Stopped::<B> {
            process_builder,
            ctx,
            snd,
            // default restart policy to prevent automatic restarts
            restart: RestartPolicy::new(BackoffStrategy::None, Vec::new()),
        }
    }
}

impl SupervisorRunner<Stopped> {
    pub fn new<B>(
        process_builder: B,
        id: String,
        ctx: Context<bool>,
        snd: Sender<Event>,
    ) -> SupervisorRunner<Stopped<B>>
    where
        B: CommandBuilder,
    {
        SupervisorRunner {
            state: Stopped::new(process_builder, ctx, snd),
            id,
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
pub(crate) mod sleep_supervisor_tests {
    use std::sync::mpsc::Sender;

    use crate::{
        command::{processrunner::ProcessRunnerBuilder, stream::Event},
        context::Context,
    };

    use super::{Stopped, SupervisorRunner};

    pub(crate) fn new_sleep_supervisor(
        tx: Sender<Event>,
        seconds: u32,
    ) -> SupervisorRunner<Stopped> {
        SupervisorRunner::new(
            ProcessRunnerBuilder::new(
                "sh".to_owned(),
                vec!["-c".to_string(), format!("sleep {}", seconds)],
            ),
            "sleep/test".to_string(),
            Context::new(),
            tx.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::stream::OutputEvent;
    use crate::supervisor::restart::Backoff;
    use std::time::{Duration, Instant};

    #[test]
    fn test_supervisor_retries_and_exits_on_wrong_command() {
        let (tx, _) = std::sync::mpsc::channel();

        let backoff = Backoff::new()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let agent: SupervisorRunner = SupervisorRunner::new(
            ProcessRunnerBuilder::new("wrong-command".to_owned(), vec!["x".to_owned()]),
            "test/wrong_command".to_string(),
            Context::new(),
            tx,
        )
        .with_restart_policy(vec![0], BackoffStrategy::Fixed(backoff));

        let agent = agent.run();

        while !agent.handle.is_finished() {
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

        let agent: SupervisorRunner = SupervisorRunner::new(
            ProcessRunnerBuilder::new("wrong-command".to_owned(), vec!["x".to_owned()]),
            "test/wrong_command".to_string(),
            Context::new(),
            tx,
        )
        .with_restart_policy(vec![0], BackoffStrategy::Fixed(backoff));

        // run the agent with wrong command so it enters in restart policy
        let agent = agent.run();
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

        let agent: SupervisorRunner = SupervisorRunner::new(
            ProcessRunnerBuilder::new("echo".to_owned(), vec!["Hello!".to_owned()]),
            "test/retry".to_string(),
            Context::new(),
            tx,
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

        let stdout = stream.join().unwrap();

        // 1 base execution + 3 retries
        assert_eq!(4, stdout.iter().count());
    }
}
