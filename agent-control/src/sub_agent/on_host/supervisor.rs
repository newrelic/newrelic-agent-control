use crate::agent_control::agent_id::AgentID;
use crate::agent_type::runtime_config::health_config::rendered::OnHostHealthConfig;
use crate::agent_type::runtime_config::on_host::filesystem::rendered::FileSystemEntries;
use crate::agent_type::runtime_config::version_config::rendered::OnHostVersionConfig;
use crate::event::SubAgentInternalEvent;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::{EventConsumer, EventPublisher, pub_sub};
use crate::health::health_checker::{HealthCheckerError, spawn_health_checker};
use crate::health::health_checker::{Healthy, Unhealthy};
use crate::health::on_host::health_checker::OnHostHealthCheckers;
use crate::health::with_start_time::{HealthWithStartTime, StartTime};
use crate::http::client::HttpClient;
use crate::http::config::{HttpConfig, ProxyConfig};
use crate::sub_agent::identity::{AgentIdentity, ID_ATTRIBUTE_NAME};
use crate::sub_agent::on_host::command::command_os::{CommandOSNotStarted, CommandOSStarted};
use crate::sub_agent::on_host::command::error::CommandError;
use crate::sub_agent::on_host::command::executable_data::ExecutableData;
use crate::sub_agent::on_host::command::restart_policy::RestartPolicy;
use crate::sub_agent::supervisor::starter::{SupervisorStarter, SupervisorStarterError};
use crate::sub_agent::supervisor::stopper::SupervisorStopper;
use crate::utils::thread_context::{
    NotStartedThreadContext, StartedThreadContext, ThreadContextStopperError,
};
use crate::version_checker::onhost::{OnHostAgentVersionChecker, check_version};
use fs::LocalFile;
use fs::directory_manager::DirectoryManagerFs;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::{Duration, SystemTime};
use tracing::{debug, error, info, info_span, warn};

pub struct StartedSupervisorOnHost {
    thread_contexts: Vec<StartedThreadContext>,
}

pub struct NotStartedSupervisorOnHost {
    agent_identity: AgentIdentity,
    executables: Vec<ExecutableData>,
    log_to_file: bool,
    logging_path: PathBuf,
    health_config: OnHostHealthConfig,
    version_config: Option<OnHostVersionConfig>,
    filesystem_entries: FileSystemEntries,
}

impl SupervisorStarter for NotStartedSupervisorOnHost {
    type SupervisorStopper = StartedSupervisorOnHost;

    fn start(
        self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Result<Self::SupervisorStopper, SupervisorStarterError> {
        let (health_publisher, health_consumer) = pub_sub();

        // Write the files required for this sub-agent to disk.

        self.filesystem_entries
            .write(&LocalFile, &DirectoryManagerFs)
            .map_err(SupervisorStarterError::FileSystem)?;

        let executable_thread_contexts = self
            .executables
            .iter()
            .map(|e| self.start_process_thread(e, health_publisher.clone()));

        self.check_subagent_version(sub_agent_internal_publisher.clone());

        let thread_contexts: Vec<StartedThreadContext> =
            vec![self.start_health_check(sub_agent_internal_publisher.clone(), health_consumer)?]
                .into_iter()
                .flatten()
                .collect();

        let thread_contexts = executable_thread_contexts
            .into_iter()
            .chain(thread_contexts)
            .collect();

        Ok(StartedSupervisorOnHost { thread_contexts })
    }
}

impl SupervisorStopper for StartedSupervisorOnHost {
    fn stop(self) -> Result<(), ThreadContextStopperError> {
        let mut stop_result = Ok(());
        for thread_context in self.thread_contexts {
            let thread_name = thread_context.thread_name().to_string();
            match thread_context.stop_blocking() {
                Ok(_) => info!("{} stopped", thread_name),
                Err(error_msg) => {
                    error!("Stopping '{thread_name}': {error_msg}");
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
        executables: Vec<ExecutableData>,
        health_config: OnHostHealthConfig,
        version_config: Option<OnHostVersionConfig>,
    ) -> Self {
        NotStartedSupervisorOnHost {
            agent_identity,
            executables,
            log_to_file: false,
            logging_path: PathBuf::default(),
            health_config,
            version_config,
            filesystem_entries: FileSystemEntries::default(),
        }
    }

    pub fn with_filesystem_entries(self, filesystem_entries: FileSystemEntries) -> Self {
        Self {
            filesystem_entries,
            ..self
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
        health_consumer: EventConsumer<(String, HealthWithStartTime)>,
    ) -> Result<Option<StartedThreadContext>, SupervisorStarterError> {
        let start_time = StartTime::now();
        let client_timeout = Duration::from(self.health_config.clone().timeout);
        let http_config = HttpConfig::new(client_timeout, client_timeout, ProxyConfig::default());
        let http_client = HttpClient::new(http_config).map_err(|err| {
            HealthCheckerError::Generic(format!("could not build the http client: {err}"))
        })?;

        let health_checker = OnHostHealthCheckers::try_new(
            health_consumer,
            http_client,
            self.health_config.check.clone(),
            start_time,
        )?;

        let started_thread_context = spawn_health_checker(
            self.agent_identity.id.clone(),
            health_checker,
            sub_agent_internal_publisher,
            self.health_config.interval,
            self.health_config.initial_delay,
            start_time,
        );
        Ok(Some(started_thread_context))
    }

    pub fn check_subagent_version(
        &self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) {
        let Some(version_config) = &self.version_config else {
            info!(agent_type=%self.agent_identity.agent_type_id, "Version checks are disabled for this agent");
            return;
        };

        let onhost_version_checker = OnHostAgentVersionChecker {
            path: version_config.path.clone(),
            args: version_config.args.clone(),
            regex: version_config.regex.clone(),
        };

        check_version(
            self.agent_identity.id.to_string(),
            onhost_version_checker,
            sub_agent_internal_publisher,
            // The below argument expects a function "AgentVersion -> T"
            // where T is the "event" sendable by the above publisher.
            // Using an enum variant that wraps a type is the same as a function taking the type.
            // Basically, it's the same as passing "|x| SubAgentInternalEvent::AgentVersionInfo(x)"
            SubAgentInternalEvent::AgentVersionInfo,
        )
    }

    fn start_process_thread(
        &self,
        executable_data: &ExecutableData,
        health_publisher: EventPublisher<(String, HealthWithStartTime)>,
    ) -> StartedThreadContext {
        let mut restart_policy = executable_data.restart_policy.clone();

        let exec_data = executable_data.clone();
        let mut health_handler = HealthHandler::new(exec_data.clone(), health_publisher.clone());

        let agent_id = self.agent_identity.id.clone();
        let not_started_executable = NotStartedExecutable::new(
            agent_id.clone(),
            exec_data.clone(),
            self.log_to_file,
            self.logging_path.clone(),
        );

        let callback = move |stop_consumer: EventConsumer<CancellationMessage>| {
            let mut i = 0;
            loop {
                let span = info_span!("executable process", { ID_ATTRIBUTE_NAME } = %agent_id, exec_data.id);
                let span = span.entered();

                // Check if we need to cancel the process before even getting started.
                // Otherwise, we would always execute the command at least once. This
                // might have unintended consequences. For example, modifying files in the system
                // that shouldn't have been modified.
                if stop_consumer.is_cancelled() {
                    debug!("Supervisor stopped before starting executable");
                    break;
                }

                health_handler.set_time(SystemTime::now());

                // TODO: when the executable fails, and max-retries are not configured in the backoff policy, this
                // can lead to false positives (reporting healthy when the executable is actually not working)
                debug!("Informing executable as healthy");
                health_handler.publish_healthy();

                let started = not_started_executable.launch();
                let executable_result =
                    started.and_then(|executable| executable.wait_for_exit(&stop_consumer));
                match executable_result {
                    Ok((exit_status, was_cancelled)) => {
                        if !exit_status.success() {
                            let ExecutableData { bin, args, .. } = &exec_data;
                            error!(%agent_id,supervisor = bin,exit_code = ?exit_status.code(),"Executable exited unsuccessfully");
                            debug!(%exit_status, "Error executing executable, marking as unhealthy");

                            let args = args.join(" ");
                            let error = format!(
                                "path '{bin}' with args '{args}' failed with '{exit_status}'",
                            );
                            let status = format!(
                                "process exited with code: {:?}",
                                exit_status.code().unwrap_or_default()
                            );
                            health_handler.publish_unhealthy_with_status(error, status);
                        }

                        if was_cancelled {
                            span.exit();
                            break;
                        }
                    }
                    Err(err) => {
                        error!(supervisor = exec_data.bin, "Launching executable: {err}");
                        debug!("Error launching executable, marking as unhealthy");
                        health_handler.publish_unhealthy(format!("Error launching process: {err}"));
                    }
                }

                info!(msg = "Executable not running");

                if !restart_policy.should_retry() {
                    warn!("Restart policy exceeded, executable won't restart anymore");
                    debug!("Restart policy exceeded, marking as unhealthy");
                    health_handler.publish_unhealthy("Restart policy exceeded".to_string());
                    span.exit();
                    break;
                }

                i += 1;
                if !restart_process_thread(&mut restart_policy, i, &stop_consumer) {
                    span.exit();
                    break;
                }

                span.exit();
            }
        };

        NotStartedThreadContext::new(executable_data.bin.clone(), callback).start()
    }
}

/// Waits for the restart policy backoff timeout to complete
///
/// If the [`CancellationMessage`] while waiting, the restart will be aborted.
fn restart_process_thread(
    restart_policy: &mut RestartPolicy,
    step: u32,
    stop_consumer: &EventConsumer<CancellationMessage>,
) -> bool {
    let max_retries = restart_policy.backoff.max_retries();
    info!("Restarting supervisor ({step}/{max_retries})");

    let mut restart = true;
    restart_policy.backoff(|duration| {
        // early exit if supervisor timeout is canceled
        if stop_consumer.is_cancelled_with_timeout(duration) {
            restart = false;
        }
    });

    restart
}

struct NotStartedExecutable {
    agent_id: AgentID,
    exec_data: ExecutableData,
    log_to_file: bool,
    logging_path: PathBuf,
}

impl NotStartedExecutable {
    fn new(
        agent_id: AgentID,
        exec_data: ExecutableData,
        log_to_file: bool,
        logging_path: PathBuf,
    ) -> Self {
        Self {
            agent_id,
            exec_data,
            log_to_file,
            logging_path,
        }
    }

    fn launch(&self) -> Result<StartedExecutable, CommandError> {
        info!("Starting executable");
        let command = CommandOSNotStarted::new(
            self.agent_id.clone(),
            &self.exec_data,
            self.log_to_file,
            self.logging_path.clone(),
        );

        let command = command.start().and_then(|cmd| cmd.stream())?;

        Ok(StartedExecutable {
            bin: self.exec_data.bin.clone(),
            command,
        })
    }
}

struct StartedExecutable {
    bin: String,
    command: CommandOSStarted,
}

impl StartedExecutable {
    fn wait_for_exit(
        mut self,
        stop_consumer: &EventConsumer<CancellationMessage>,
    ) -> Result<(ExitStatus, bool), CommandError> {
        info!("Waiting for executable to complete or be cancelled");
        let mut was_cancelled = false;

        // Listen for the cancellation signal while the command is running.
        // Upon receiving the signal, we kill the process. That way, we can ensure
        // that the thread stops in time.
        while self.command.is_running() {
            if stop_consumer.is_cancelled() {
                info!(supervisor = self.bin, "Stopping executable");
                let _ = self.command.shutdown();
                info!(supervisor = self.bin, msg = "Executable terminated");
                was_cancelled = true;
            }
        }

        // At this point, the command is already dead. However, we call `wait` to
        // release resources.
        // Reference - https://doc.rust-lang.org/std/process/struct.Child.html#warning
        self.command
            .wait()
            .map(|exit_status| (exit_status, was_cancelled))
    }
}

struct HealthHandler {
    exec_data: ExecutableData,
    health_publisher: EventPublisher<(String, HealthWithStartTime)>,
    time: SystemTime,
}

impl HealthHandler {
    fn new(
        exec_data: ExecutableData,
        health_publisher: EventPublisher<(String, HealthWithStartTime)>,
    ) -> Self {
        Self {
            exec_data,
            health_publisher,
            time: SystemTime::now(),
        }
    }

    fn set_time(&mut self, time: SystemTime) {
        self.time = time;
    }

    fn publish_healthy(&self) {
        let id = self.exec_data.id.to_string();
        let health = HealthWithStartTime::new(Healthy::new().into(), self.time);
        if let Err(err) = self.health_publisher.publish((id.clone(), health)) {
            error!("Publishing health status for {id}: {err}");
        }
    }

    fn publish_unhealthy(&self, error: String) {
        let id = self.exec_data.id.to_string();
        let unhealthy = HealthWithStartTime::new(Unhealthy::new(error).into(), self.time);
        if let Err(err) = self.health_publisher.publish((id.clone(), unhealthy)) {
            error!("Publishing health status for {id}: {err}");
        }
    }

    fn publish_unhealthy_with_status(&self, error: String, status: String) {
        let id = self.exec_data.id.to_string();
        let unhealthy =
            HealthWithStartTime::new(Unhealthy::new(error).with_status(status).into(), self.time);
        if let Err(err) = self.health_publisher.publish((id.clone(), unhealthy)) {
            error!("Publishing health status for {id}: {err}");
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::runtime_config::health_config::rendered;
    use crate::event::channel::pub_sub;
    use crate::health::health_checker::HEALTH_CHECKER_THREAD_NAME;
    use crate::sub_agent::on_host::command::executable_data::ExecutableData;
    use crate::sub_agent::on_host::command::restart_policy::BackoffStrategy;
    use crate::sub_agent::on_host::command::restart_policy::{Backoff, RestartPolicy};
    use rstest::*;
    use std::thread;
    use std::time::{Duration, Instant};
    use tracing_test::internal::logs_with_scope_contain;
    use tracing_test::traced_test;

    #[cfg(target_family = "unix")] //TODO This should be removed when Windows support is added
    #[traced_test]
    #[rstest]
    #[case::long_running_process_shutdown_after_start(
        "long-running",
        ExecutableData::new("sleep".to_owned(), "sleep".to_owned()).with_args(vec!["10".to_owned()]),
        Some(Duration::from_secs(1)),
        vec!["Stopping executable", "Executable terminated"])]
    #[case::fail_process_shutdown_after_start(
        "wrong-command",
        ExecutableData::new("wrong-command".to_owned(), "wrong-command".to_owned()),
        Some(Duration::from_secs(1)),
        vec!["Executable not running"])]
    fn test_supervisor_gracefully_shutdown(
        #[case] agent_id: &str,
        #[case] executable: ExecutableData,
        #[case] run_warmup_time: Option<Duration>,
        #[case] contain_logs: Vec<&'static str>,
    ) {
        let backoff = Backoff::default()
            .with_initial_delay(Duration::from_secs(5))
            .with_max_retries(1);
        let executable_data = vec![
            executable.with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff))),
        ];

        let agent_identity = AgentIdentity::from((
            agent_id.to_owned().try_into().unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let supervisor = NotStartedSupervisorOnHost::new(
            agent_identity,
            executable_data,
            OnHostHealthConfig::default(),
            None,
        );

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

        let max_duration = Duration::from_millis(100);
        assert!(
            duration < max_duration,
            "stopping the supervisor took to much time: {duration:?}"
        );

        for log in contain_logs {
            assert!(
                tracing_test::internal::logs_with_scope_contain(
                    "newrelic_agent_control::sub_agent::on_host::supervisor",
                    log,
                ),
                "log: {log}"
            );
        }
    }

    #[test]
    fn test_supervisor_without_executables_expect_no_errors() {
        let executables = vec![];

        let agent_identity = AgentIdentity::from((
            "wrong-command".to_owned().try_into().unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let agent = NotStartedSupervisorOnHost::new(
            agent_identity,
            executables,
            OnHostHealthConfig::default(),
            None,
        );

        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();
        let agent = agent.start(sub_agent_internal_publisher).expect("no error");

        for thread_context in agent.thread_contexts {
            if thread_context.thread_name() == HEALTH_CHECKER_THREAD_NAME {
                let _ = thread_context.stop();
            } else {
                while !thread_context.is_thread_finished() {
                    thread::sleep(Duration::from_millis(15));
                }
            }
        }
    }

    #[test]
    fn test_supervisor_retries_and_exits_on_wrong_command() {
        let backoff = Backoff::default()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let executables = vec![
            ExecutableData::new("wrong-command".to_owned(), "wrong-command".to_owned())
                .with_args(vec!["x".to_owned()])
                .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff))),
        ];

        let agent_identity = AgentIdentity::from((
            "wrong-command".to_owned().try_into().unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let agent = NotStartedSupervisorOnHost::new(
            agent_identity,
            executables,
            OnHostHealthConfig::default(),
            None,
        );

        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();
        let agent = agent.start(sub_agent_internal_publisher).expect("no error");

        for thread_context in agent.thread_contexts {
            if thread_context.thread_name() == HEALTH_CHECKER_THREAD_NAME {
                let _ = thread_context.stop();
            } else {
                while !thread_context.is_thread_finished() {
                    thread::sleep(Duration::from_millis(15));
                }
            }
        }
    }

    #[test]
    #[traced_test]
    fn test_supervisor_one_wrong_command_one_correct_command() {
        let backoff = Backoff::default()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let executables = vec![
            ExecutableData::new("wrong-command".to_owned(), "wrong-command".to_owned())
                .with_args(vec!["x".to_owned()])
                .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff.clone()))),
            ExecutableData::new("echo".to_owned(), "echo".to_owned())
                .with_args(vec!["NR-command".to_owned()])
                .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff))),
        ];

        let agent_identity = AgentIdentity::from((
            "wrong-command".to_owned().try_into().unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let agent = NotStartedSupervisorOnHost::new(
            agent_identity,
            executables,
            OnHostHealthConfig::default(),
            None,
        );

        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();
        let agent = agent.start(sub_agent_internal_publisher).expect("no error");

        for thread_context in agent.thread_contexts {
            if thread_context.thread_name() == HEALTH_CHECKER_THREAD_NAME {
                let _ = thread_context.stop();
            } else {
                while !thread_context.is_thread_finished() {
                    thread::sleep(Duration::from_millis(15));
                }
            }
        }
        assert!(logs_with_scope_contain(
            "DEBUG newrelic_agent_control::sub_agent::on_host::command::logging::logger",
            "NR-command",
        ));
    }

    #[test]
    #[traced_test]
    fn test_supervisor_restart_policy_early_exit() {
        let timer = Instant::now();

        // set a fixed backoff of 10 seconds
        let backoff = Backoff::default()
            .with_initial_delay(Duration::from_secs(10))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let executables = vec![
            ExecutableData::new("wrong-command".to_owned(), "wrong-command".to_owned())
                .with_args(vec!["x".to_owned()])
                .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff))),
        ];

        let agent_identity = AgentIdentity::from((
            "wrong-command".to_owned().try_into().unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let agent = NotStartedSupervisorOnHost::new(
            agent_identity,
            executables,
            OnHostHealthConfig::default(),
            None,
        );

        // run the agent with wrong command so it enters in restart policy
        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();
        let agent = agent.start(sub_agent_internal_publisher);
        // wait two seconds to ensure restart policy thread is sleeping
        thread::sleep(Duration::from_secs(2));
        agent.expect("no error").stop().expect("no error");

        assert!(timer.elapsed() < Duration::from_secs(10));
    }

    #[test]
    #[cfg(target_family = "unix")]
    #[traced_test]
    fn test_supervisor_fixed_backoff_retry_3_times() {
        let backoff = Backoff::default()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let executables = vec![
            ExecutableData::new("echo".to_owned(), "echo".to_owned())
                .with_args(vec!["hello!".to_owned()])
                .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff))),
        ];

        let agent_identity = AgentIdentity::from((
            "echo".to_owned().try_into().unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let agent = NotStartedSupervisorOnHost::new(
            agent_identity,
            executables,
            rendered::OnHostHealthConfig::default(),
            None,
        );

        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();
        let agent = agent.start(sub_agent_internal_publisher).expect("no error");

        for thread_context in agent.thread_contexts {
            if thread_context.thread_name() == HEALTH_CHECKER_THREAD_NAME {
                let _ = thread_context.stop();
            } else {
                while !thread_context.is_thread_finished() {
                    thread::sleep(Duration::from_millis(15));
                }
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
                    "Expected 4 lines with 'hello!' corresponding to 1 run + 3 retries, got {n}"
                )),
            },
        )
        .unwrap();
    }

    #[test]
    #[cfg(target_family = "unix")]
    fn test_supervisor_health_events_on_breaking_backoff() {
        let backoff = Backoff::default()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        // FIXME using "echo 'hello!'" as a command clashes with the previous test when checking
        // the logger output. Why? See https://github.com/dbrgn/tracing-test/pull/19/ for clues.
        let executables = vec![
            ExecutableData::new("echo".to_owned(), "echo".to_owned())
                .with_args(vec!["".to_owned()])
                .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff))),
        ];

        let agent_identity = AgentIdentity::from((
            "echo".to_owned().try_into().unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let agent = NotStartedSupervisorOnHost::new(
            agent_identity,
            executables,
            OnHostHealthConfig::default(),
            None,
        );

        let (health_publisher, health_consumer) = pub_sub();

        let executable_thread_contexts = agent
            .executables
            .iter()
            .map(|e| agent.start_process_thread(e, health_publisher.clone()));

        for thread_context in executable_thread_contexts {
            while !thread_context.is_thread_finished() {
                thread::sleep(Duration::from_millis(15));
            }
        }

        // Fix the start times to allow comparison
        let start_time = SystemTime::now();

        // It starts once and restarts 3 times, hence 4 healthy events and a final unhealthy one
        let expected_ordered_events: Vec<(String, HealthWithStartTime)> = [
            (
                "echo".to_owned(),
                HealthWithStartTime::new(Healthy::new().into(), start_time),
            ),
            (
                "echo".to_owned(),
                HealthWithStartTime::new(Healthy::new().into(), start_time),
            ),
            (
                "echo".to_owned(),
                HealthWithStartTime::new(Healthy::new().into(), start_time),
            ),
            (
                "echo".to_owned(),
                HealthWithStartTime::new(Healthy::new().into(), start_time),
            ),
            (
                "echo".to_owned(),
                HealthWithStartTime::new(
                    Unhealthy::new("Restart policy exceeded".to_string()).into(),
                    start_time,
                ),
            ),
        ]
        .into_iter()
        .collect();

        let actual_ordered_events = health_consumer
            .as_ref()
            .try_iter()
            .map(|event| {
                // Patch start_time for health events to allow comparison
                (
                    event.0.clone(),
                    HealthWithStartTime::new(event.1.into(), start_time),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(actual_ordered_events, expected_ordered_events);
    }
}
