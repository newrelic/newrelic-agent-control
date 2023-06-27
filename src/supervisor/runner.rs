use std::process::ExitStatus;
use std::{
    ops::Deref,
    sync::mpsc::Sender,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

use crate::command::error::CommandError;
use crate::command::processrunner::ProcessRunnerBuilder;
use crate::command::shutdown::ProcessTerminatorBuilder;
use crate::command::{CommandBuilder, TerminatorBuilder};
use crate::{
    command::{
        stream::{Event, Metadata},
        wait_exit_timeout, wait_exit_timeout_default, CommandExecutor, CommandHandle,
        CommandTerminator, EventStreamer, ProcessTerminator,
    },
    context::Context,
};

use super::{
    error::ProcessError,
    restart::{BackoffStrategy, RestartPolicy},
    Handle, Runner, ID,
};

use tracing::{error, info};

pub struct Stopped<B, T>
where
    B: CommandBuilder,
    T: TerminatorBuilder,
{
    process_builder: B,
    terminator: Arc<Mutex<T>>,
    ctx: Context<bool>,
    snd: Sender<Event>,
    restart: RestartPolicy,
}

pub struct Running {
    handle: JoinHandle<()>,
    ctx: Context<bool>,
}

#[derive(Debug)]
pub struct SupervisorRunner<State = Stopped<ProcessRunnerBuilder, ProcessTerminatorBuilder>> {
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

impl<B: CommandBuilder, T: TerminatorBuilder> ID for SupervisorRunner<Stopped<B, T>> {
    fn id(&self) -> String {
        self.id.clone()
    }
}

impl<B: CommandBuilder + 'static, T: TerminatorBuilder + 'static> Runner
    for SupervisorRunner<Stopped<B, T>>
{
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

impl<B: CommandBuilder, T: TerminatorBuilder> From<&SupervisorRunner<Stopped<B, T>>> for Metadata {
    fn from(value: &SupervisorRunner<Stopped<B, T>>) -> Self {
        Metadata::new(value.id())
    }
}

// launch_process starts a new process with a streamed channel and sets its current pid
// into the provided variable. It waits until the process exits.
fn launch_process<C: CommandExecutor>(
    executor: C,
    pid: Arc<Mutex<Option<u32>>>,
    tx: Sender<Event>,
) -> Result<ExitStatus, CommandError> {
    // run and stream the process
    let streaming = executor.start()?.stream(tx)?;

    // set current running pid
    *pid.lock().unwrap() = Some(streaming.get_pid());

    Ok(streaming.wait().unwrap())
}

fn run_process_thread<B: CommandBuilder + 'static, T: TerminatorBuilder + 'static>(
    runner: SupervisorRunner<Stopped<B, T>>,
) -> JoinHandle<()> {
    let mut restart_policy = runner.restart.clone();
    let current_pid: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));

    let shutdown_ctx = Context::new();
    // _ = wait_for_termination(
    //     current_pid.clone(),
    //     runner.ctx.clone(),
    //     shutdown_ctx.clone(),
    // );
    thread::spawn({
        let ctx = runner.ctx.clone();
        let shutdown_ctx = shutdown_ctx.clone();
        let current_pid = current_pid.clone();
        let terminator_builder = runner.terminator.clone();
        move || {
            let (lck, cvar) = Context::get_lock_cvar(&ctx);
            _ = cvar.wait_while(lck.lock().unwrap(), |finish| !*finish);

            if let Some(pid) = *current_pid.lock().unwrap() {
                terminator_builder.lock().unwrap().with_pid(pid);
                let _ = terminator_builder
                    .lock()
                    .unwrap()
                    .build()
                    .shutdown(|| wait_exit_timeout_default(shutdown_ctx));
            }
        }
    });
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
                runner.process_builder.build(),
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

impl<B, T> Stopped<B, T>
where
    B: CommandBuilder,
    T: TerminatorBuilder,
{
    fn new(
        custom_builder: B,
        custom_terminator: T,
        ctx: Context<bool>,
        snd: Sender<Event>,
    ) -> Stopped<B, T>
    where
        B: CommandBuilder,
        T: TerminatorBuilder,
    {
        Stopped::<B, T> {
            process_builder: custom_builder,
            terminator: Arc::new(Mutex::new(custom_terminator)),
            ctx,
            snd,
            restart: RestartPolicy::new(BackoffStrategy::None, Vec::new()),
        }
    }
}

impl<B, T> SupervisorRunner<Stopped<B, T>>
where
    B: CommandBuilder,
    T: TerminatorBuilder,
{
    #[cfg(test)]
    pub fn new_with_builder_and_terminator(
        process_builder: B,
        process_terminator: T,
        id: String,
        ctx: Context<bool>,
        snd: Sender<Event>,
    ) -> SupervisorRunner<Stopped<B, T>>
    where
        B: CommandBuilder,
    {
        SupervisorRunner {
            state: Stopped::new(process_builder, process_terminator, ctx, snd),
            id,
        }
    }
}

impl SupervisorRunner<Stopped<ProcessRunnerBuilder, ProcessTerminatorBuilder>> {
    pub fn new(
        bin: String,
        args: Vec<String>,
        id: String,
        ctx: Context<bool>,
        snd: Sender<Event>,
    ) -> SupervisorRunner<Stopped<ProcessRunnerBuilder, ProcessTerminatorBuilder>> {
        SupervisorRunner {
            state: Stopped::new(
                ProcessRunnerBuilder::new(bin, args),
                ProcessTerminatorBuilder::default(),
                ctx,
                snd,
            ),
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
    use std::{
        sync::{atomic::AtomicBool, mpsc::Sender, Arc},
        time::Duration,
    };

    use crate::{
        command::{
            processrunner::sleep_process_builder::MockedProcessBuilder,
            shutdown::terminator_builder::NopTerminatorBuiler, stream::Event,
        },
        context::Context,
    };

    use super::{Stopped, SupervisorRunner};

    pub(crate) fn new_sleep_supervisor(
        tx: Sender<Event>,
        seconds: u32,
    ) -> SupervisorRunner<Stopped<MockedProcessBuilder, NopTerminatorBuiler>> {
        let release = Arc::new(AtomicBool::new(false));
        SupervisorRunner::new_with_builder_and_terminator(
            MockedProcessBuilder::new(false, Duration::from_secs(seconds as u64), release.clone()),
            NopTerminatorBuiler::new(release),
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
            "wrong-command".to_owned(),
            vec!["x".to_owned()],
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
            "wrong-command".to_owned(),
            vec!["x".to_owned()],
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
            "echo".to_owned(),
            vec!["Hello!".to_owned()],
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
