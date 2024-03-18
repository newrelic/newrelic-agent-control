use crate::context::Context;

use crate::event::channel::EventPublisher;
use crate::event::SubAgentInternalEvent;
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
use crate::sub_agent::restart_policy::BackoffStrategy;
use crate::super_agent::config::AgentID;
use std::process::ExitStatus;
use std::{
    ops::Deref,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};
use tracing::{error, info, warn};

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

    pub fn id(&self) -> AgentID {
        self.state.config.id.clone()
    }

    pub fn bin(&self) -> String {
        self.state.config.bin.clone()
    }

    pub fn logs_to_file(&self) -> bool {
        self.state.config.log_to_file
    }

    pub fn run(
        self,
        internal_event_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Result<SupervisorOnHost<Started>, SupervisorError> {
        //TODO: validate binary path, exec permissions...?
        let ctx = self.state.config.ctx.clone();
        Ok(SupervisorOnHost {
            state: Started {
                handle: start_process_thread(self, internal_event_publisher),
                ctx,
            },
        })
    }

    pub fn not_started_command(&self) -> CommandOS<command_os::NotStarted> {
        //TODO extract to to a builder so we can mock it
        CommandOS::<command_os::NotStarted>::new(
            self.state.config.id.clone(),
            &self.state.config.bin,
            &self.state.config.args,
            &self.state.config.env,
            self.logs_to_file(),
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
) -> Result<ExitStatus, CommandError>
where
    R: NotStartedCommand,
{
    // run and stream the process
    let started = not_started_command.start()?;

    let streaming = started.stream()?;

    // set current running pid
    *pid.lock().unwrap() = Some(streaming.get_pid());

    streaming.wait()
}

fn start_process_thread(
    not_started_supervisor: SupervisorOnHost<NotStarted>,
    internal_event_publisher: EventPublisher<SubAgentInternalEvent>,
) -> JoinHandle<()> {
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
                id = not_started_supervisor.id().to_string(),
                supervisor = not_started_supervisor.bin(),
                msg = "Starting supervisor process"
            );

            shutdown_ctx.reset().unwrap();
            // Signals return exit_code 0, if in the future we need to act on them we can import
            // std::os::unix::process::ExitStatusExt to get the code with the method into_raw
            let not_started_command = not_started_supervisor.not_started_command();
            let bin = not_started_supervisor.bin();
            let id = not_started_supervisor.id();

            publish_health_event(
                &internal_event_publisher,
                SubAgentInternalEvent::AgentBecameHealthy,
            );

            let exit_code = start_command(not_started_command, current_pid.clone())
                .inspect_err(|err| {
                    error!(
                        id = id.to_string(),
                        supervisor = bin,
                        "Error while launching supervisor process: {}",
                        err
                    );
                })
                .map(|exit_code| {
                    if !exit_code.success() {
                        publish_health_event(
                            &internal_event_publisher,
                            SubAgentInternalEvent::AgentBecameUnhealthy(format!(
                                "process exited with code: {}",
                                exit_code
                            )),
                        );
                        error!(
                            id = id.to_string(),
                            supervisor = bin,
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
                // Log if we are not restarting anymore due to the restart policy being broken
                if restart_policy.backoff != BackoffStrategy::None {
                    warn!("Supervisor for {id} won't restart anymore due to having exceeded its restart policy");
                    publish_health_event(
                        &internal_event_publisher,
                        SubAgentInternalEvent::AgentBecameUnhealthy(
                            "supervisor exceeded its defined restart policy".to_string(),
                        ),
                    );
                }
                break;
            }

            info!("Restarting supervisor for {id}...");

            restart_policy.backoff(|duration| {
                // early exit if supervisor timeout is canceled
                wait_exit_timeout(not_started_supervisor.ctx.clone(), duration);
            });
        }
    })
}

fn publish_health_event(
    internal_event_publisher: &EventPublisher<SubAgentInternalEvent>,
    event: SubAgentInternalEvent,
) {
    _ = internal_event_publisher
        .publish(event)
        .inspect_err(|e| error!(err = e.to_string(), "could not publish sub agent event"));
}

/// Blocks on the [`Context`], [`ctx`]. When the termination signal is activated, this will send a shutdown signal to the process being supervised (the one whose PID was passed as [`pid`]).
fn wait_for_termination(
    current_pid: Arc<Mutex<Option<u32>>>,
    ctx: Context<bool>,
    shutdown_ctx: Context<bool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let (lck, cvar) = Context::get_lock_cvar(&ctx);
        drop(cvar.wait_while(lck.lock().unwrap(), |finish| !*finish));

        if let Some(pid) = *current_pid.lock().unwrap() {
            _ = ProcessTerminator::new(pid).shutdown(|| wait_exit_timeout_default(shutdown_ctx));
        }
    })
}

#[cfg(test)]
pub mod sleep_supervisor_tests {

    use super::SupervisorOnHost;
    use super::{NotStarted, SupervisorConfigOnHost};
    use crate::context::Context;
    use crate::sub_agent::on_host::supervisor::command_supervisor_config::ExecutableData;
    use crate::sub_agent::restart_policy::{BackoffStrategy, RestartPolicy};

    pub fn new_sleep_supervisor(seconds: u32) -> SupervisorOnHost<NotStarted> {
        let exec = ExecutableData::new("sh".to_owned())
            .with_args(vec!["-c".to_owned(), format!("sleep {}", seconds)]);
        let config = SupervisorConfigOnHost::new(
            "sleep-supervisor".to_owned().try_into().unwrap(),
            exec,
            Context::new(),
            RestartPolicy::new(BackoffStrategy::None, Vec::new()),
            false,
        );
        SupervisorOnHost::new(config)
    }
}

#[cfg(test)]
mod tests {
    use tracing_test::traced_test;

    use super::*;
    use crate::event::channel::pub_sub;
    use crate::sub_agent::on_host::supervisor::command_supervisor_config::ExecutableData;
    use crate::sub_agent::restart_policy::{Backoff, BackoffStrategy, RestartPolicy};
    use std::time::{Duration, Instant};

    #[test]
    fn test_supervisor_retries_and_exits_on_wrong_command() {
        let backoff = Backoff::new()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let exec = ExecutableData::new("wrong-command".to_owned()).with_args(vec!["x".to_owned()]);

        let config = SupervisorConfigOnHost::new(
            "wrong-command".to_owned().try_into().unwrap(),
            exec,
            Context::new(),
            RestartPolicy::new(BackoffStrategy::Fixed(backoff), vec![0]),
            false,
        );
        let agent = SupervisorOnHost::new(config);

        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();
        let agent = agent.run(sub_agent_internal_publisher).unwrap();

        while !agent.state.handle.is_finished() {
            thread::sleep(Duration::from_millis(15));
        }
    }

    #[test]
    fn test_supervisor_restart_policy_early_exit() {
        let timer = Instant::now();

        // set a fixed backoff of 10 seconds
        let backoff = Backoff::new()
            .with_initial_delay(Duration::from_secs(10))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let exec = ExecutableData::new("wrong-command".to_owned()).with_args(vec!["x".to_owned()]);

        let config = SupervisorConfigOnHost::new(
            "wrong-command".to_owned().try_into().unwrap(),
            exec,
            Context::new(),
            RestartPolicy::new(BackoffStrategy::Fixed(backoff), vec![0]),
            false,
        );
        let agent = SupervisorOnHost::new(config);

        // run the agent with wrong command so it enters in restart policy
        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();
        let agent = agent.run(sub_agent_internal_publisher).unwrap();
        // wait two seconds to ensure restart policy thread is sleeping
        thread::sleep(Duration::from_secs(2));
        assert!(agent.stop().join().is_ok());

        assert!(timer.elapsed() < Duration::from_secs(10));
    }

    #[test]
    #[traced_test]
    fn test_supervisor_fixed_backoff_retry_3_times() {
        let backoff = Backoff::new()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let exec = ExecutableData::new("echo".to_owned()).with_args(vec!["hello!".to_owned()]);

        let config = SupervisorConfigOnHost::new(
            "echo".to_owned().try_into().unwrap(),
            exec,
            Context::new(),
            RestartPolicy::new(BackoffStrategy::Fixed(backoff), vec![0]),
            false,
        );
        let agent = SupervisorOnHost::new(config);

        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();
        let agent = agent.run(sub_agent_internal_publisher).unwrap();

        while !agent.state.handle.is_finished() {
            thread::sleep(Duration::from_millis(15));
        }

        // Log output corresponding to 1 base execution + 3 retries
        tracing_test::internal::logs_assert(
            "DEBUG newrelic_super_agent::sub_agent::on_host::command::logging::logger",
            |lines| match lines.iter().filter(|line| line.contains("hello!")).count() {
                4 => Ok(()),
                n => Err(format!(
                    "Expected 4 lines with 'hello!' corresponding to 1 run + 3 retries, got {}",
                    n
                )),
            },
        )
        .unwrap();
    }
}
