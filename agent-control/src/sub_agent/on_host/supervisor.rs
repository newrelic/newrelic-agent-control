use crate::agent_control::agent_id::AgentID;
use crate::agent_type::runtime_config::health_config::rendered::OnHostHealthConfig;
use crate::agent_type::runtime_config::on_host::filesystem::rendered::FileSystemEntries;
use crate::agent_type::runtime_config::on_host::rendered::RenderedPackages;
use crate::agent_type::runtime_config::version_config::rendered::OnHostVersionConfig;
use crate::checkers::health::health_checker::{Health, HealthCheckerError, spawn_health_checker};
use crate::checkers::health::health_checker::{Healthy, Unhealthy};
use crate::checkers::health::on_host::health_checker::OnHostHealthCheckers;
use crate::checkers::health::with_start_time::{HealthWithStartTime, StartTime};
use crate::checkers::version::onhost::{OnHostAgentVersionChecker, check_version};
use crate::event::SubAgentInternalEvent;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::{EventConsumer, EventPublisher, pub_sub};
use crate::http::client::HttpClient;
use crate::http::config::{HttpConfig, ProxyConfig};
use crate::package::manager::{PackageData, PackageManager};
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::on_host::command::command_os::{CommandOSNotStarted, CommandOSStarted};
use crate::sub_agent::on_host::command::error::CommandError;
use crate::sub_agent::on_host::command::executable_data::ExecutableData;
use crate::sub_agent::on_host::command::restart_policy::RestartPolicy;
use crate::sub_agent::supervisor::starter::{SupervisorStarter, SupervisorStarterError};
use crate::sub_agent::supervisor::stopper::SupervisorStopper;
use crate::utils::thread_context::{
    NotStartedThreadContext, StartedThreadContext, ThreadContextStopperError,
};
use fs::directory_manager::DirectoryManagerFs;
use fs::file::LocalFile;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tracing::{Dispatch, debug, dispatcher, error, info, warn};

const WAIT_FOR_EXIT_TIMEOUT: Duration = Duration::from_secs(1);
const HEALTHY_DELAY: Duration = Duration::from_secs(10);

pub struct StartedSupervisorOnHost {
    thread_contexts: Vec<StartedThreadContext>,
}

pub struct NotStartedSupervisorOnHost<PM>
where
    PM: PackageManager,
{
    agent_identity: AgentIdentity,
    executables: Vec<ExecutableData>,
    log_to_file: bool,
    logging_path: PathBuf,
    health_config: OnHostHealthConfig,
    package_manager: Arc<PM>,
    packages_config: RenderedPackages,
    version_config: Option<OnHostVersionConfig>,
    filesystem_entries: FileSystemEntries,
}

impl<PM> SupervisorStarter for NotStartedSupervisorOnHost<PM>
where
    PM: PackageManager,
{
    type SupervisorStopper = StartedSupervisorOnHost;

    fn start(
        self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Result<Self::SupervisorStopper, SupervisorStarterError> {
        let (health_publisher, health_consumer) = pub_sub();

        for (id, package) in &self.packages_config {
            // Currently we are always installing the package without checking if it's already installed.
            debug!(%id, "Installing package");
            self.package_manager
                .install(
                    &self.agent_identity.id,
                    PackageData {
                        id: id.clone(),
                        package_type: package.package_type.clone(),
                        oci_reference: package.download.oci.reference.clone(),
                    },
                )
                .map_err(|err| SupervisorStarterError::InstallPackage(err.to_string()))?;
            debug!(%id, "Package successfully installed");
        }

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

impl<PM> NotStartedSupervisorOnHost<PM>
where
    PM: PackageManager,
{
    pub fn new(
        agent_identity: AgentIdentity,
        executables: Vec<ExecutableData>,
        health_config: OnHostHealthConfig,
        version_config: Option<OnHostVersionConfig>,
        packages: RenderedPackages,
        package_manager: Arc<PM>,
    ) -> Self {
        NotStartedSupervisorOnHost {
            agent_identity,
            executables,
            log_to_file: false,
            logging_path: PathBuf::default(),
            health_config,
            package_manager,
            packages_config: packages,
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
            // The below argument expects a function "UpdateAttributesMessage -> T"
            // where T is the "event" sendable by the above publisher.
            // Using an enum variant that wraps a type is the same as a function taking the type.
            // Basically, it's the same as passing "|x| SubAgentInternalEvent::UpdateAttributesMessage(x)"
            SubAgentInternalEvent::AgentAttributesUpdated,
        )
    }

    fn start_process_thread(
        &self,
        executable_data: &ExecutableData,
        health_publisher: EventPublisher<(String, HealthWithStartTime)>,
    ) -> StartedThreadContext {
        let mut restart_policy = executable_data.restart_policy.clone();
        let exec_data = executable_data.clone();
        let agent_id = self.agent_identity.id.clone();
        let log_to_file = self.log_to_file;
        let logging_path = self.logging_path.clone();

        let dispatch = dispatcher::get_default(|d: &Dispatch| d.clone());
        let span = tracing::Span::current();

        let callback = move |stop_consumer: EventConsumer<CancellationMessage>| {
            let _guard = dispatcher::set_default(&dispatch);
            let _enter = span.enter();

            let exec_id = exec_data.id.clone();

            let mut i = 0;
            loop {
                // Check if we need to cancel the process before even getting started.
                // Otherwise, we would always execute the command at least once. This
                // might have unintended consequences. For example, modifying files in the system
                // that shouldn't have been modified.
                if stop_consumer.is_cancelled() {
                    debug!(%agent_id, %exec_id, "Supervisor stopped before starting executable");
                    break;
                }

                // It's important to create a new health handler for each process instance
                // Otherwise, the published time won't be updated.
                let health_handler = HealthHandler::new(exec_id.clone(), health_publisher.clone());

                info!(%agent_id, %exec_id, "Starting executable");
                let command = CommandOSNotStarted::new(
                    agent_id.clone(),
                    &exec_data,
                    log_to_file,
                    logging_path.clone(),
                );

                let started = command.start().and_then(|cmd| cmd.stream());

                let executable_result = started.and_then(|cmd| {
                    wait_exit(
                        cmd,
                        &stop_consumer,
                        HEALTHY_DELAY,
                        &health_handler,
                        &agent_id,
                        &exec_id,
                    )
                });

                match executable_result {
                    Ok((exit_status, was_cancelled)) => {
                        handle_exit(&agent_id, &exec_data, &exit_status, &health_handler);

                        if was_cancelled {
                            break;
                        }
                    }
                    Err(err) => {
                        warn!(%agent_id, %exec_id, "Launching executable: {err}");
                        debug!(%agent_id, %exec_id, "Error launching executable, marking as unhealthy");
                        health_handler.publish_unhealthy(format!("Error launching process: {err}"));
                    }
                }

                info!(%agent_id, %exec_id, "Executable not running");

                if !restart_policy.should_retry() {
                    warn!(%agent_id, %exec_id, "Restart policy exceeded, executable won't restart anymore");
                    debug!(%agent_id, %exec_id, "Restart policy exceeded, marking as unhealthy");
                    health_handler.publish_unhealthy("Restart policy exceeded".to_string());
                    break;
                }

                i += 1;
                let restart_cancelled = wait_restart(&mut restart_policy, i, &stop_consumer);
                if restart_cancelled {
                    break;
                }
            }
        };

        NotStartedThreadContext::new(executable_data.bin.clone(), callback).start()
    }
}

/// Waits for the command to complete or be cancelled
fn wait_exit(
    mut command: CommandOSStarted,
    stop_consumer: &EventConsumer<CancellationMessage>,
    healthy_publish_delay: Duration,
    health_handler: &HealthHandler,
    agent_id: &AgentID,
    exec_id: &str,
) -> Result<(ExitStatus, bool), CommandError> {
    info!(%agent_id, %exec_id, "Waiting for executable to complete or be cancelled");
    let mut was_cancelled = false;
    let deadline = Instant::now() + healthy_publish_delay;
    let mut healthy_already_published = false;

    // Busy waiting is avoided with `is_cancelled_with_timeout`
    while command.is_running() {
        // Shutdown the spawned process when the cancel signal is received.
        // This ensures the thread stops in time.
        if stop_consumer.is_cancelled_with_timeout(WAIT_FOR_EXIT_TIMEOUT) {
            info!(%agent_id, %exec_id, "Stopping executable");
            if let Err(err) = command.shutdown() {
                error!(%agent_id, %exec_id, "Failed to stop executable: {err}");
            }
            info!(%agent_id, %exec_id, "Executable terminated");
            was_cancelled = true;
        }

        // Publish healthy status once after the process has been running
        // for an arbitrary long time without issues.
        if !healthy_already_published && Instant::now() > deadline {
            debug!(%agent_id, %exec_id, "{}", format!("Informing executable as healthy after running for {} seconds", healthy_publish_delay.as_secs()));
            health_handler.publish_healthy();
            healthy_already_published = true;
        }
    }

    // At this point, the command is already dead. However, we call `wait` to
    // release resources.
    // Reference - https://doc.rust-lang.org/std/process/struct.Child.html#warning
    command
        .wait()
        .inspect(|exit_status| {
            if !healthy_already_published && exit_status.success() {
                debug!(%agent_id, %exec_id, "Informing executable as healthy after terminating successfully");
                health_handler.publish_healthy();
            }
        })
        .map(|exit_status| (exit_status, was_cancelled))
}

/// Waits for the restart policy backoff timeout and returns whether it was cancelled or not
fn wait_restart(
    restart_policy: &mut RestartPolicy,
    step: u32,
    stop_consumer: &EventConsumer<CancellationMessage>,
) -> bool {
    let max_retries = restart_policy.backoff.max_retries();
    info!("Waiting for restart policy backoff");

    let mut cancelled = false;
    restart_policy.backoff(|duration| {
        // early exit if supervisor timeout is canceled
        if stop_consumer.is_cancelled_with_timeout(duration) {
            cancelled = true;
        }
    });

    if !cancelled {
        info!("Restarting supervisor ({step}/{max_retries})");
    } else {
        info!("Restarting supervisor ({step}/{max_retries}) was cancelled");
    }

    cancelled
}

/// Executes operations based on the exit status of the command
fn handle_exit(
    agent_id: &AgentID,
    exec_data: &ExecutableData,
    exit_status: &ExitStatus,
    health_handler: &HealthHandler,
) {
    if exit_status.success() {
        return;
    }

    let ExecutableData { bin, args, .. } = &exec_data;
    warn!(%agent_id,supervisor = bin,exit_code = ?exit_status.code(),"Executable exited unsuccessfully");
    debug!(%exit_status, "Error executing executable, marking as unhealthy");

    let args = args.join(" ");
    let error = format!("path '{bin}' with args '{args}' failed with '{exit_status}'",);
    let status = format!(
        "process exited with code: {}",
        exit_status.code().unwrap_or_default()
    );
    health_handler.publish_unhealthy_with_status(error, status);
}

#[derive(Clone)]
struct HealthHandler {
    id: String,
    health_publisher: EventPublisher<(String, HealthWithStartTime)>,
    time: SystemTime,
}

impl HealthHandler {
    fn new(id: String, health_publisher: EventPublisher<(String, HealthWithStartTime)>) -> Self {
        Self {
            id,
            health_publisher,
            time: SystemTime::now(),
        }
    }

    fn publish_healthy(&self) {
        self.publish_health(Healthy::new().into());
    }

    fn publish_unhealthy(&self, error: String) {
        self.publish_health(Unhealthy::new(error).into());
    }

    fn publish_unhealthy_with_status(&self, error: String, status: String) {
        self.publish_health(Unhealthy::new(error).with_status(status).into());
    }

    fn publish_health(&self, health: Health) {
        let health = HealthWithStartTime::new(health, self.time);
        if let Err(err) = self.health_publisher.publish((self.id.clone(), health)) {
            error!("Publishing health status for {}: {err}", self.id);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::checkers::health::health_checker::HEALTH_CHECKER_THREAD_NAME;
    use crate::event::channel::pub_sub;
    use crate::package::manager::tests::MockPackageManager;
    use crate::sub_agent::on_host::command::executable_data::ExecutableData;
    use crate::sub_agent::on_host::command::restart_policy::BackoffStrategy;
    use crate::sub_agent::on_host::command::restart_policy::{Backoff, RestartPolicy};
    use serde::Deserialize;
    use std::collections::HashMap;
    use std::thread;
    use std::time::{Duration, Instant};
    use tracing_test::traced_test;

    #[derive(Clone, Deserialize)]
    struct TextExecutableData {
        id: String,
        path: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    }

    impl From<TextExecutableData> for ExecutableData {
        fn from(text_exec_data: TextExecutableData) -> Self {
            ExecutableData::new(text_exec_data.id, text_exec_data.path)
                .with_args(text_exec_data.args)
                .with_env(text_exec_data.env)
        }
    }

    fn build_test_exec_data(json_str: &str) -> ExecutableData {
        serde_json::from_str::<TextExecutableData>(json_str)
            .expect("Test input should be deserializable to an ExecutableData")
            .into()
    }

    #[cfg(target_family = "unix")]
    #[traced_test]
    #[rstest::rstest]
    #[cfg_attr(target_family = "unix",case::long_running_process_shutdown_after_start(
        "long-running",
        build_test_exec_data(r#"{"id":"sleep","path":"sleep","args":["10"]}"#),
        Some(Duration::from_secs(1)),
        vec!["Stopping executable", "Executable terminated"]))]
    #[cfg_attr(target_family = "windows",case::long_running_process_shutdown_after_start(
        "long-running",
        build_test_exec_data(r#"{"id":"cmd","path":"cmd","args":["/C","timeout","/T","10","/NOBREAK"]}"#),
        Some(Duration::from_secs(1)),
        vec!["Stopping executable", "Executable terminated"]))]
    #[case::fail_process_shutdown_after_start(
        "wrong-command",
        build_test_exec_data(r#"{"id":"wrong-command","path":"wrong-command"}"#),
        Some(Duration::from_secs(1)),
        vec!["Executable not running"])]
    fn test_supervisor_gracefully_shutdown(
        #[case] agent_id: &str,
        #[case] executable: ExecutableData,
        #[case] run_warmup_time: Option<Duration>,
        #[case] contain_logs: Vec<&'static str>,
    ) {
        const DURATION_DELTA: Duration = Duration::from_millis(100);

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
            get_empty_packages(),
            MockPackageManager::new_arc(),
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

        let max_duration = WAIT_FOR_EXIT_TIMEOUT + DURATION_DELTA;
        assert!(
            duration < max_duration,
            "stopping the supervisor took to much time: {duration:?}"
        );

        for log in contain_logs {
            assert!(logs_contain(log), "log not found: {log}");
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
            get_empty_packages(),
            MockPackageManager::new_arc(),
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
            build_test_exec_data(r#"{"id":"wrong-command","path":"wrong-command","args":["x"]}"#)
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
            get_empty_packages(),
            MockPackageManager::new_arc(),
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
            build_test_exec_data(r#"{"id":"wrong-command","path":"wrong-command","args":["x"]}"#)
                .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff.clone()))),
            #[cfg(target_family = "unix")]
            build_test_exec_data(r#"{"id":"echo","path":"echo","args":["NR-command"]}"#)
                .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff))),
            #[cfg(target_family = "windows")]
            build_test_exec_data(r#"{"id":"cmd","path":"cmd","args":["/C","echo","NR-command"]}"#)
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
            get_empty_packages(),
            MockPackageManager::new_arc(),
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
        assert!(logs_contain("NR-command"));
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
            build_test_exec_data(r#"{"id":"wrong-command","path":"wrong-command","args":["x"]}"#)
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
            get_empty_packages(),
            MockPackageManager::new_arc(),
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
    #[traced_test]
    fn test_supervisor_fixed_backoff_retry_3_times() {
        let backoff = Backoff::default()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let executables = vec![
            #[cfg(target_family = "unix")]
            build_test_exec_data(r#"{"id":"echo","path":"echo","args":["hello!"]}"#)
                .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff))),
            #[cfg(target_family = "windows")]
            build_test_exec_data(r#"{"id":"cmd","path":"cmd","args":["/C","echo","hello!"]}"#)
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
            get_empty_packages(),
            MockPackageManager::new_arc(),
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
            "newrelic_agent_control::sub_agent::on_host::command::logging::logger",
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
    fn test_supervisor_health_events_on_breaking_backoff() {
        let backoff = Backoff::default()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let exec_id = "echo-process";

        // FIXME using "echo 'hello!'" as a command clashes with the previous test when checking
        // the logger output. Why? See https://github.com/dbrgn/tracing-test/pull/19/ for clues.
        let executables = vec![
            #[cfg(target_family = "unix")]
            build_test_exec_data(r#"{"id":"echo-process","path":"echo","args":[""]}"#)
                .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff))),
            #[cfg(target_family = "windows")]
            build_test_exec_data(r#"{"id":"echo-process","path":"cmd","args":["/C","echo",""]}"#)
                .with_restart_policy(RestartPolicy::new(BackoffStrategy::Fixed(backoff))),
        ];

        let agent_identity = AgentIdentity::from((
            exec_id.to_owned().try_into().unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let agent = NotStartedSupervisorOnHost::new(
            agent_identity,
            executables,
            OnHostHealthConfig::default(),
            None,
            get_empty_packages(),
            MockPackageManager::new_arc(),
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
                exec_id.to_string(),
                HealthWithStartTime::new(Healthy::new().into(), start_time),
            ),
            (
                exec_id.to_string(),
                HealthWithStartTime::new(Healthy::new().into(), start_time),
            ),
            (
                exec_id.to_string(),
                HealthWithStartTime::new(Healthy::new().into(), start_time),
            ),
            (
                exec_id.to_string(),
                HealthWithStartTime::new(Healthy::new().into(), start_time),
            ),
            (
                exec_id.to_string(),
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

    #[test]
    fn test_wait_on_exit_publish_healthy_once() {
        #[cfg(target_family = "unix")]
        let exec_data = ExecutableData::new("sleep".to_owned(), "sleep".to_owned())
            .with_args(vec!["3".to_owned()]);
        #[cfg(target_family = "windows")]
        let exec_data = ExecutableData::new("sleep".to_owned(), "timeout".to_owned())
            .with_args(vec!["/T".to_owned(), "3".to_owned(), "/NOBREAK".to_owned()]);

        let agent_id = AgentID::AgentControl;
        let command = CommandOSNotStarted::new(agent_id.clone(), &exec_data, false, PathBuf::new())
            .start()
            .unwrap();

        let (health_publisher, health_consumer) = pub_sub();
        let health_handler = HealthHandler::new(exec_data.id.clone(), health_publisher);

        // Don't use the "_" expression for the publisher.
        // Renaming it to "_" drops the channel. Hence, it will be disconnected.
        // `wait_for_exit` then gets out on the first iteration and this test will
        // always pass even when it shouldn't.
        let (_stop_publisher, stop_consumer) = pub_sub::<CancellationMessage>();
        let _ = wait_exit(
            command,
            &stop_consumer,
            Duration::ZERO,
            &health_handler,
            &agent_id,
            &exec_data.id,
        );

        let start_time = SystemTime::now();
        let expected_ordered_events = vec![(
            "sleep".to_owned(),
            HealthWithStartTime::new(Healthy::new().into(), start_time),
        )];

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

    #[test]
    fn test_wait_on_exit_no_publish() {
        #[cfg(target_family = "unix")]
        let exec_data = ExecutableData::new("ls".to_owned(), "ls".to_owned())
            .with_args(vec!["non-existent-path".to_owned()]);
        #[cfg(target_family = "windows")]
        let exec_data = ExecutableData::new("cmd".to_owned(), "cmd".to_owned()).with_args(vec![
            "/C".to_owned(),
            "dir".to_owned(),
            "non-existent-path".to_owned(),
        ]);

        let agent_id = AgentID::AgentControl;
        let command = CommandOSNotStarted::new(agent_id.clone(), &exec_data, false, PathBuf::new())
            .start()
            .unwrap();

        let (health_publisher, health_consumer) = pub_sub();
        let health_handler = HealthHandler::new(exec_data.id.clone(), health_publisher);

        let (_stop_publisher, stop_consumer) = pub_sub::<CancellationMessage>();
        let _ = wait_exit(
            command,
            &stop_consumer,
            Duration::from_secs(10),
            &health_handler,
            &agent_id,
            &exec_data.id,
        );

        assert!(health_consumer.as_ref().is_empty())
    }

    fn get_empty_packages() -> RenderedPackages {
        HashMap::new()
    }
}
