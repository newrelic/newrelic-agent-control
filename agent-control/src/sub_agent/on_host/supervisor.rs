use crate::agent_control::agent_id::AgentID;
use crate::agent_type::runtime_config::health_config::OnHostHealthConfig;
use crate::agent_type::version_config::VersionCheckerInterval;
use crate::context::Context;
use crate::event::channel::EventPublisher;
use crate::event::SubAgentInternalEvent;
use crate::http::client::HttpClient;
use crate::http::config::{HttpConfig, ProxyConfig};
use crate::sub_agent::health::health_checker::{
    publish_health_event, spawn_health_checker, HealthCheckerError,
};
use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
use crate::sub_agent::health::on_host::health_checker::OnHostHealthChecker;
use crate::sub_agent::health::with_start_time::{HealthWithStartTime, StartTime};
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::on_host::command::command::CommandError;
use crate::sub_agent::on_host::command::command_os::CommandOSNotStarted;
use crate::sub_agent::on_host::command::executable_data::ExecutableData;
use crate::sub_agent::on_host::command::restart_policy::BackoffStrategy;
use crate::sub_agent::on_host::command::shutdown::{
    wait_exit_timeout, wait_exit_timeout_default, ProcessTerminator,
};
use crate::sub_agent::supervisor::starter::{SupervisorStarter, SupervisorStarterError};
use crate::sub_agent::supervisor::stopper::SupervisorStopper;
use crate::sub_agent::version::onhost::OnHostAgentVersionChecker;
use crate::sub_agent::version::version_checker::spawn_version_checker;
use crate::utils::thread_context::{
    NotStartedThreadContext, StartedThreadContext, ThreadContextStopperError,
};
use crate::utils::threads::spawn_named_thread;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::{Duration, SystemTime};
use std::{
    sync::{Arc, Mutex},
    thread::JoinHandle,
};
use tracing::{debug, error, info, info_span, warn};

pub struct StartedSupervisorOnHost {
    ctx: Context<bool>,
    thread_contexts: Vec<StartedThreadContext>,
}

pub struct NotStartedSupervisorOnHost {
    pub(super) agent_identity: AgentIdentity,
    pub(super) ctx: Context<bool>,
    pub(crate) maybe_exec: Option<ExecutableData>,
    pub(super) log_to_file: bool,
    pub(super) logging_path: PathBuf,
    pub(super) health_config: Option<OnHostHealthConfig>,
}

impl SupervisorStarter for NotStartedSupervisorOnHost {
    type SupervisorStopper = StartedSupervisorOnHost;

    fn start(
        self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Result<Self::SupervisorStopper, SupervisorStarterError> {
        let ctx = self.ctx.clone();

        let thread_contexts = vec![
            self.start_health_check(sub_agent_internal_publisher.clone())?,
            self.start_version_checker(sub_agent_internal_publisher.clone()),
            // the process thread is created if exec is Some
            self.maybe_exec
                .clone()
                .map(|e| self.start_process_thread(sub_agent_internal_publisher, e)),
        ];

        Ok(StartedSupervisorOnHost {
            ctx,
            thread_contexts: thread_contexts.into_iter().flatten().collect(),
        })
    }
}

impl SupervisorStopper for StartedSupervisorOnHost {
    fn stop(self) -> Result<(), ThreadContextStopperError> {
        self.ctx.cancel_all(true).unwrap();

        let mut stop_result = Ok(());
        for thread_context in self.thread_contexts {
            let thread_name = thread_context.thread_name().to_string();
            match thread_context.stop_blocking() {
                Ok(_) => info!("{} stopped", thread_name),
                Err(error_msg) => {
                    error!("Error stopping '{thread_name}': {error_msg}");
                    if stop_result.is_ok() {
                        stop_result = Err(error_msg);
                    }
                }
            }
        }

        stop_result
    }
}

impl NotStartedSupervisorOnHost {
    pub fn new(
        agent_identity: AgentIdentity,
        maybe_exec: Option<ExecutableData>,
        ctx: Context<bool>,
        health_config: Option<OnHostHealthConfig>,
    ) -> Self {
        NotStartedSupervisorOnHost {
            agent_identity,
            ctx,
            maybe_exec,
            log_to_file: false,
            logging_path: PathBuf::default(),
            health_config,
        }
    }

    pub fn with_file_logging(self, log_to_file: bool, logging_path: PathBuf) -> Self {
        Self {
            log_to_file,
            logging_path,
            ..self
        }
    }

    fn start_health_check(
        &self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Result<Option<StartedThreadContext>, SupervisorStarterError> {
        let start_time = StartTime::now();
        if let Some(health_config) = &self.health_config {
            let client_timeout = Duration::from(health_config.clone().timeout);
            let http_config =
                HttpConfig::new(client_timeout, client_timeout, ProxyConfig::default());
            let http_client = HttpClient::new(http_config).map_err(|err| {
                HealthCheckerError::Generic(format!("could not build the http client: {err}"))
            })?;

            let health_checker =
                OnHostHealthChecker::try_new(http_client, health_config.clone(), start_time)?;
            let started_thread_context = spawn_health_checker(
                health_checker,
                sub_agent_internal_publisher,
                health_config.interval,
                start_time,
            );
            return Ok(Some(started_thread_context));
        }
        debug!("health checks are disabled for this agent");
        Ok(None)
    }

    pub fn start_version_checker(
        &self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Option<StartedThreadContext> {
        let onhost_version_checker =
            OnHostAgentVersionChecker::checked_new(self.agent_identity.agent_type_id.clone())?;

        Some(spawn_version_checker(
            onhost_version_checker,
            sub_agent_internal_publisher,
            VersionCheckerInterval::default(),
        ))
    }

    fn start_process_thread(
        self,
        internal_event_publisher: EventPublisher<SubAgentInternalEvent>,
        executable_data: ExecutableData,
    ) -> StartedThreadContext {
        let mut restart_policy = executable_data.restart_policy.clone();
        let current_pid: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
        let shutdown_ctx = Context::new();
        _ = wait_for_termination(current_pid.clone(), self.ctx.clone(), shutdown_ctx.clone());

        let executable_data_clone = executable_data.clone();
        // NotStartedThreadContext takes as input a callback that requires a EventConsumer<CancellationMessage>
        // as input. In that specific case it's not used, but we need to pass it to comply with the signature.
        // This should be refactored to work as the other threads used by the supervisor.
        let callback = move |_| loop {
            // locks the current_pid to prevent `wait_for_termination` finishing before the process
            // is started and the pid is set.
            // In case starting the process fail the guard will be dropped and `wait_for_termination`
            // will finish without needing to cancel any process (current_pid==None).
            let pid_guard: std::sync::MutexGuard<Option<u32>> = current_pid.lock().unwrap();

            // A context cancelled means that the supervisor has been gracefully stopped
            // before the process was started.
            if *Context::get_lock_cvar(&self.ctx).0.lock().unwrap() {
                debug!(
                    supervisor = executable_data_clone.bin,
                    msg = "supervisor stopped before starting the process"
                );
                break;
            }

            info!(
                supervisor = executable_data_clone.bin,
                msg = "starting supervisor process"
            );

            shutdown_ctx.reset().unwrap();
            // Signals return exit_code 0, if in the future we need to act on them we can import
            // std::os::unix::process::ExitStatusExt to get the code with the method into_raw
            let not_started_command = self.not_started_command(&executable_data_clone);

            let supervisor_start_time = SystemTime::now();

            let init_health = Healthy::new(String::default());

            publish_health_event(
                &internal_event_publisher,
                HealthWithStartTime::new(init_health.into(), supervisor_start_time).into(),
            );

            let exit_code = start_command(not_started_command, pid_guard)
                .inspect_err(|err| {
                    error!(
                        supervisor = executable_data_clone.bin,
                        "error while launching supervisor process: {}", err
                    );
                })
                .map(|exit_status| {
                    handle_termination(
                        exit_status,
                        &internal_event_publisher,
                        &self.agent_identity.id,
                        executable_data_clone.bin.to_string(),
                        supervisor_start_time,
                    )
                });

            // A context cancelled means that the supervisor has been gracefully stopped and is the
            // most probably reason why process has been exited.
            if *Context::get_lock_cvar(&self.ctx).0.lock().unwrap() {
                info!(
                    supervisor = executable_data_clone.bin,
                    msg = "supervisor has been stopped and process terminated"
                );
                break;
            }

            // canceling the shutdown ctx must be done before getting current_pid lock
            // as it locked by the wait_for_termination function
            shutdown_ctx.cancel_all(true).unwrap();
            *current_pid.lock().unwrap() = None;

            // check if restart policy needs to be applied
            // As the exit code comes inside a Result but we don't care about the Err,
            // we just unwrap or take the default value (0)
            if !restart_policy.should_retry(exit_code.unwrap_or_default()) {
                // Log if we are not restarting anymore due to the restart policy being broken
                if restart_policy.backoff != BackoffStrategy::None {
                    warn!("supervisor won't restart anymore due to having exceeded its restart policy");

                    let unhealthy = Unhealthy::new(
                        String::default(),
                        "supervisor exceeded its defined restart policy".to_string(),
                    );

                    publish_health_event(
                        &internal_event_publisher,
                        HealthWithStartTime::new(unhealthy.into(), supervisor_start_time).into(),
                    );
                }
                break;
            }

            info!("restarting supervisor");

            restart_policy.backoff(|duration| {
                // early exit if supervisor timeout is canceled
                wait_exit_timeout(self.ctx.clone(), duration);
            });
        };

        NotStartedThreadContext::new(executable_data.bin, callback).start()
    }

    pub fn not_started_command(&self, executable_data: &ExecutableData) -> CommandOSNotStarted {
        //TODO extract to to a builder so we can mock it
        CommandOSNotStarted::new(
            self.agent_identity.id.clone(),
            executable_data,
            self.log_to_file,
            self.logging_path.clone(),
        )
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Helpers (TODO: Review and move?)
////////////////////////////////////////////////////////////////////////////////////

/// From the `ExitStatus`, send appropriate event and emit logs, return exit code.
fn handle_termination(
    exit_status: ExitStatus,
    internal_event_publisher: &EventPublisher<SubAgentInternalEvent>,
    agent_id: &AgentID,
    bin: String,
    start_time: SystemTime,
) -> i32 {
    if !exit_status.success() {
        let unhealthy: Unhealthy = Unhealthy::new(
            format!(
                "process exited with code: {:?}",
                exit_status.code().unwrap_or_default()
            ),
            exit_status.to_string(),
        );
        publish_health_event(
            internal_event_publisher,
            HealthWithStartTime::new(unhealthy.into(), start_time).into(),
        );
        error!(
            %agent_id,
            supervisor = bin,
            exit_code = ?exit_status.code(),
            "supervisor process exited unsuccessfully"
        )
    }
    // From the docs on `ExitStatus::code()`: "On Unix, this will return `None` if the process was terminated by a signal."
    // Since we need to act on this exit code irrespective of it coming from a signal or not, we try to get the code,
    // falling back to getting the signal if not, and finally to 0 if both fail.
    let exit_code = exit_status.code();
    let exit_signal = exit_status.signal();

    // If in the future we need to act differently on signals, we can return a sum type that
    // can contain either an exit code or a signal, has a sensible default for our use case,
    // and have `RestartPolicy::should_retry` handle it.
    exit_code.or(exit_signal).unwrap_or_default()
}

/// launch_process starts a new process with a streamed channel and sets its current pid
/// into the provided variable. It waits until the process exits.
fn start_command(
    not_started_command: CommandOSNotStarted,
    mut pid: std::sync::MutexGuard<Option<u32>>,
) -> Result<ExitStatus, CommandError> {
    // run and stream the process
    let started = not_started_command.start()?;

    let streaming = started.stream()?;

    // set current running pid
    *pid = Some(streaming.get_pid());
    // free the lock so the wait_for_termination can lock it on graceful shutdown
    drop(pid);

    streaming.wait()
}

/// Blocks on the [`Context`], [`ctx`]. When the termination signal is activated, this will send
/// a shutdown signal to the process being supervised (the one whose PID was passed as [`pid`]).
fn wait_for_termination(
    current_pid: Arc<Mutex<Option<u32>>>,
    ctx: Context<bool>,
    shutdown_ctx: Context<bool>,
) -> JoinHandle<()> {
    let s = info_span!("termination_signal_listener");
    spawn_named_thread("OnHost Termination signal listener", move || {
        let _guards = s.enter();
        let (lck, cvar) = Context::get_lock_cvar(&ctx);
        drop(cvar.wait_while(lck.lock().unwrap(), |finish| !*finish));

        // context is unlocked here so locking it again in other thread that is blocking current_pid is safe.

        match *current_pid.lock().unwrap() { Some(pid) => {
            info!(pid = pid, msg = "stopping supervisor process");
            _ = ProcessTerminator::new(pid).shutdown(|| wait_exit_timeout_default(shutdown_ctx));
        } _ => {
            info!(msg = "stopped supervisor without process running");
        }}
    })
}

#[cfg(test)]
pub mod tests {
    use rstest::*;

    use super::*;

    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::context::Context;
    use crate::event::channel::pub_sub;
    use crate::sub_agent::health::health_checker::Healthy;
    use crate::sub_agent::on_host::command::executable_data::ExecutableData;
    use crate::sub_agent::on_host::command::restart_policy::{Backoff, RestartPolicy};
    use crate::sub_agent::version::version_checker::AgentVersion;
    use std::thread;
    use std::time::{Duration, Instant};
    use tracing_test::traced_test;

    #[cfg(unix)]
    #[traced_test]
    #[rstest]
    #[case::long_running_process_shutdown_after_start(
        "long-running",
        ExecutableData::new("sleep".to_owned()).with_args(vec!["10".to_owned()]),
        Some(Duration::from_secs(1)),
        vec!["stopping supervisor process", "supervisor has been stopped and process terminated"])]
    #[case::fail_process_shutdown_after_start(
        "wrong-command",
        ExecutableData::new("wrong-command".to_owned()),
        Some(Duration::from_secs(1)),
        vec!["stopped supervisor without process running"])]
    #[case::long_running_process_shutdown_before_start(
        "long-running-before-start",
        ExecutableData::new("sleep".to_owned()).with_args(vec!["10".to_owned()]),
        None,
        vec!["supervisor stopped before starting the process", "stopped supervisor without process running"])]
    fn test_supervisor_gracefully_shutdown(
        #[case] agent_id: &str,
        #[case] executable: ExecutableData,
        #[case] run_warmup_time: Option<Duration>,
        #[case] contain_logs: Vec<&'static str>,
    ) {
        let backoff = Backoff::new()
            .with_initial_delay(Duration::from_secs(5))
            .with_max_retries(1);
        let any_exit_code = vec![];
        let executable_data = Some(executable.with_restart_policy(RestartPolicy::new(
            BackoffStrategy::Fixed(backoff),
            any_exit_code,
        )));

        let agent_identity = AgentIdentity::from((
            agent_id.to_owned().try_into().unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let ctx = Context::<bool>::new();
        if agent_id == "long-running-before-start" {
            ctx.cancel_all(true).unwrap();
        }
        let supervisor =
            NotStartedSupervisorOnHost::new(agent_identity, executable_data, ctx, None);

        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();

        let started_supervisor = supervisor.start(sub_agent_internal_publisher);
        if let Some(duration) = run_warmup_time {
            thread::sleep(duration)
        }

        // stopping the agent should be instantaneous since terminating sleep is fast.
        // no restarts should occur.
        let start = Instant::now();
        started_supervisor.expect("no error").stop().unwrap();
        let duration = start.elapsed();

        // gives the `wait_for_termination` thread time to finish.
        thread::sleep(Duration::from_secs(1));

        let max_duration = Duration::from_millis(100);
        assert!(
            duration < max_duration,
            "stopping the supervisor took to much time: {:?}",
            duration
        );

        for log in contain_logs {
            assert!(
                tracing_test::internal::logs_with_scope_contain(
                    "newrelic_agent_control::sub_agent::on_host::supervisor",
                    log,
                ),
                "log: {}",
                log
            );
        }
    }

    #[test]
    fn test_supervisor_retries_and_exits_on_wrong_command() {
        let backoff = Backoff::new()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let exec = ExecutableData::new("wrong-command".to_owned())
            .with_args(vec!["x".to_owned()])
            .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff), vec![0]));

        let agent_identity = AgentIdentity::from((
            "wrong-command".to_owned().try_into().unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let agent =
            NotStartedSupervisorOnHost::new(agent_identity, Some(exec), Context::new(), None);

        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();
        let agent = agent.start(sub_agent_internal_publisher).expect("no error");

        for thread_context in agent.thread_contexts {
            while !thread_context.is_thread_finished() {
                thread::sleep(Duration::from_millis(15));
            }
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

        let exec = ExecutableData::new("wrong-command".to_owned())
            .with_args(vec!["x".to_owned()])
            .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff), vec![0]));

        let agent_identity = AgentIdentity::from((
            "wrong-command".to_owned().try_into().unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let agent =
            NotStartedSupervisorOnHost::new(agent_identity, Some(exec), Context::new(), None);

        // run the agent with wrong command so it enters in restart policy
        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();
        let agent = agent.start(sub_agent_internal_publisher);
        // wait two seconds to ensure restart policy thread is sleeping
        thread::sleep(Duration::from_secs(2));
        agent.expect("no error").stop().expect("no error");

        assert!(timer.elapsed() < Duration::from_secs(10));
    }

    #[test]
    #[cfg(unix)]
    #[traced_test]
    fn test_supervisor_fixed_backoff_retry_3_times() {
        let backoff = Backoff::new()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let exec = ExecutableData::new("echo".to_owned())
            .with_args(vec!["hello!".to_owned()])
            .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff), vec![0]));

        let agent_identity = AgentIdentity::from((
            "echo".to_owned().try_into().unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let agent =
            NotStartedSupervisorOnHost::new(agent_identity, Some(exec), Context::new(), None);

        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();
        let agent = agent.start(sub_agent_internal_publisher).expect("no error");

        for thread_context in agent.thread_contexts {
            while !thread_context.is_thread_finished() {
                thread::sleep(Duration::from_millis(15));
            }
        }

        // buffer to ensure all logs are flushed
        thread::sleep(Duration::from_millis(300));

        // Log output corresponding to 1 base execution + 3 retries
        tracing_test::internal::logs_assert(
            "DEBUG newrelic_agent_control::sub_agent::on_host::command::logging::logger",
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

    #[test]
    #[cfg(unix)]
    fn test_supervisor_health_events_on_breaking_backoff() {
        let backoff = Backoff::new()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        // FIXME using "echo 'hello!'" as a command clashes with the previous test when checking
        // the logger output. Why? See https://github.com/dbrgn/tracing-test/pull/19/ for clues.
        let exec = ExecutableData::new("echo".to_owned())
            .with_args(vec!["".to_owned()])
            .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff), vec![0]));

        let agent_identity = AgentIdentity::from((
            "echo".to_owned().try_into().unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let agent =
            NotStartedSupervisorOnHost::new(agent_identity, Some(exec), Context::new(), None);

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let agent = agent.start(sub_agent_internal_publisher).expect("no error");

        for thread_context in agent.thread_contexts {
            while !thread_context.is_thread_finished() {
                thread::sleep(Duration::from_millis(15));
            }
        }

        // Fix the start times to allow comparison
        let start_time = SystemTime::now();

        // It starts once and restarts 3 times, hence 4 healthy events and a final unhealthy one
        let expected_ordered_events: Vec<SubAgentInternalEvent> = {
            vec![
                HealthWithStartTime::new(Healthy::default().into(), start_time).into(),
                HealthWithStartTime::new(Healthy::default().into(), start_time).into(),
                HealthWithStartTime::new(Healthy::default().into(), start_time).into(),
                HealthWithStartTime::new(Healthy::default().into(), start_time).into(),
                HealthWithStartTime::new(
                    Unhealthy::new(
                        String::default(),
                        "supervisor exceeded its defined restart policy".to_string(),
                    )
                    .into(),
                    start_time,
                )
                .into(),
            ]
        };

        let actual_ordered_events = sub_agent_internal_consumer
            .as_ref()
            .iter()
            .map(|event| match event {
                SubAgentInternalEvent::AgentHealthInfo(health) => {
                    HealthWithStartTime::new(health.into(), start_time).into()
                }
                SubAgentInternalEvent::StopRequested => SubAgentInternalEvent::StopRequested,
                SubAgentInternalEvent::AgentVersionInfo(agent_id) => {
                    SubAgentInternalEvent::AgentVersionInfo(AgentVersion::new(
                        agent_id.version().to_string(),
                        agent_id.opamp_field().to_string(),
                    ))
                }
            })
            .collect::<Vec<_>>();

        assert_eq!(actual_ordered_events, expected_ordered_events);
    }
}
