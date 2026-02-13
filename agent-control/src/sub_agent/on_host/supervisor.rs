use crate::agent_control::agent_id::AgentID;
use crate::agent_type::runtime_config::health_config::rendered::OnHostHealthConfig;
use crate::agent_type::runtime_config::on_host::filesystem::rendered::{
    FileSystem, FileSystemEntriesError,
};
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
use crate::sub_agent::effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssemblerError};
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::on_host::command::command_os::{CommandOSNotStarted, CommandOSStarted};
use crate::sub_agent::on_host::command::error::CommandError;
use crate::sub_agent::on_host::command::executable_data::ExecutableData;
use crate::sub_agent::on_host::command::restart_policy::RestartPolicy;
use crate::sub_agent::supervisor::{Supervisor, SupervisorStarter};
use crate::utils::thread_context::{
    NotStartedThreadContext, StartedThreadContext, ThreadContextStopperError,
};
use fs::directory_manager::DirectoryManagerFs;
use fs::file::LocalFile;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use thiserror::Error;
use tracing::{Dispatch, debug, dispatcher, error, info, warn};

const WAIT_FOR_EXIT_TIMEOUT: Duration = Duration::from_secs(1);
const HEALTHY_DELAY: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("installing packages: {0}")]
    InstallPackage(String),
    #[error("building health checkers: {0}")]
    HealthError(#[from] HealthCheckerError),
    #[error("failed to write sub-agent files: {0}")]
    FileSystem(FileSystemEntriesError),
    #[error("package installation failed: {0}")]
    Install(InstallPackageError),
    #[error("missing runtime configuration: {0}")]
    RuntimeConfig(EffectiveAgentsAssemblerError),
    #[error("failure stopping supervisor: {0}")]
    Stop(ThreadContextStopperError),
}

#[derive(Debug, Error)]
#[error("failure installing package: '{id}': {err_msg}")]
pub struct InstallPackageError {
    id: String,
    err_msg: String,
}

pub struct StartedSupervisorOnHost<PM>
where
    PM: PackageManager,
{
    pub thread_contexts: Vec<StartedThreadContext>,
    pub package_manager: Arc<PM>,
    pub agent_identity: AgentIdentity,
    pub internal_publisher: EventPublisher<SubAgentInternalEvent>,
    pub logging_path: PathBuf,
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
    filesystem: FileSystem,
}

impl<PM> SupervisorStarter for NotStartedSupervisorOnHost<PM>
where
    PM: PackageManager,
{
    type Supervisor = StartedSupervisorOnHost<PM>;
    type Error = SupervisorError;

    fn start(
        self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Result<Self::Supervisor, Self::Error> {
        install_packages(
            &self.package_manager,
            &self.agent_identity.id,
            &self.packages_config,
        )
        .map_err(SupervisorError::Install)?;

        self.spin_up(sub_agent_internal_publisher)
    }
}

impl<PM> Supervisor for StartedSupervisorOnHost<PM>
where
    PM: PackageManager,
{
    type ApplyError = SupervisorError;
    type StopError = ThreadContextStopperError;

    fn apply(self, effective_agent: EffectiveAgent) -> Result<Self, Self::ApplyError> {
        // Get configuration from effective agent
        let onhost_config = effective_agent
            .get_onhost_config()
            .map_err(SupervisorError::RuntimeConfig)?
            .clone();

        // Reuse supervisor inner fields
        let Self {
            agent_identity,
            package_manager,
            internal_publisher,
            thread_contexts,
            logging_path,
        } = self;

        let installation_result = install_packages(
            &package_manager,
            &agent_identity.id,
            &onhost_config.packages,
        );

        stop_supervisor_threads(thread_contexts).map_err(|err| {
            if let Err(err) = &installation_result {
                error!("Failure stopping supervisor. Note that installation also failed: {err}",);
            }
            SupervisorError::Stop(err)
        })?;

        installation_result.map_err(SupervisorError::Install)?;

        let executables = onhost_config
            .executables
            .into_iter()
            .map(|e| {
                ExecutableData::new(e.id, e.path)
                    .with_args(e.args.0)
                    .with_env(e.env.0)
                    .with_restart_policy(e.restart_policy.into())
            })
            .collect();

        let starter = NotStartedSupervisorOnHost::new(
            agent_identity,
            executables,
            onhost_config.health,
            onhost_config.version,
            onhost_config.packages,
            package_manager,
        )
        .with_filesystem(onhost_config.filesystem)
        .with_file_logging(onhost_config.enable_file_logging, logging_path);

        let new_started_supervisor = starter.spin_up(internal_publisher)?;

        Ok(new_started_supervisor)
    }

    fn stop(self) -> Result<(), ThreadContextStopperError> {
        stop_supervisor_threads(self.thread_contexts)
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
            filesystem: FileSystem::default(),
        }
    }

    pub fn with_filesystem(self, filesystem: FileSystem) -> Self {
        Self { filesystem, ..self }
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
    ) -> Result<Option<StartedThreadContext>, SupervisorError> {
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

    fn spin_up(
        self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Result<StartedSupervisorOnHost<PM>, SupervisorError> {
        let (health_publisher, health_consumer) = pub_sub();

        self.filesystem
            .write(&LocalFile, &DirectoryManagerFs)
            .map_err(SupervisorError::FileSystem)?;

        let executable_thread_contexts = self
            .executables
            .iter()
            .map(|e| self.start_process_thread(e, health_publisher.clone()));

        self.check_subagent_version(sub_agent_internal_publisher.clone());

        let thread_contexts =
            [self.start_health_check(sub_agent_internal_publisher.clone(), health_consumer)?]
                .into_iter()
                .flatten();

        let thread_contexts = executable_thread_contexts
            .into_iter()
            .chain(thread_contexts)
            .collect();

        Ok(StartedSupervisorOnHost {
            thread_contexts,
            package_manager: self.package_manager,
            agent_identity: self.agent_identity,
            internal_publisher: sub_agent_internal_publisher,
            logging_path: self.logging_path,
        })
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

/// Helper to install packages when starting a new supervisor or applying new configuration.
fn install_packages<PM: PackageManager>(
    package_manager: &Arc<PM>,
    agent_id: &AgentID,
    packages: &RenderedPackages,
) -> Result<(), InstallPackageError> {
    for (id, package) in packages {
        debug!(%id, "Installing package");
        package_manager
            .install(
                agent_id,
                PackageData {
                    id: id.clone(),
                    oci_reference: package.download.oci.reference.clone(),
                },
            )
            .map_err(|err| InstallPackageError {
                id: id.to_string(),
                err_msg: err.to_string(),
            })?;
        debug!(%id, "Package successfully installed");
    }
    Ok(())
}

/// Helper to stop supervisor threads logging results
fn stop_supervisor_threads(
    thread_contexts: Vec<StartedThreadContext<()>>,
) -> Result<(), ThreadContextStopperError> {
    let mut stop_result = Ok(());
    for thread_context in thread_contexts {
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
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::runtime_config::on_host::executable::rendered::{Args, Env, Executable};
    use crate::agent_type::runtime_config::on_host::rendered::OnHost;
    use crate::agent_type::runtime_config::rendered::{Deployment, Runtime};
    use crate::agent_type::runtime_config::restart_policy::rendered::RestartPolicyConfig;
    use crate::checkers::health::health_checker::HEALTH_CHECKER_THREAD_NAME;
    use crate::event::channel::pub_sub;
    use crate::package::manager::tests::MockPackageManager;
    use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
    use crate::sub_agent::on_host::command::restart_policy::BackoffStrategy;
    use crate::sub_agent::on_host::command::restart_policy::{Backoff, RestartPolicy};
    use crate::sub_agent::supervisor::Supervisor;
    use serde::Deserialize;
    use std::collections::HashMap;
    use std::fs;
    use std::thread;
    use std::time::{Duration, Instant};
    use tracing_test::traced_test;

    fn get_empty_packages() -> RenderedPackages {
        HashMap::new()
    }

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
    #[cfg_attr(target_family = "unix", case::long_running_process_shutdown_after_start(
        "long-running",
        build_test_exec_data(r#"{"id":"sleep","path":"sleep","args":["10"]}"#),
        Some(Duration::from_secs(1)),
        vec!["Stopping executable", "Executable terminated"]))]
    #[cfg_attr(target_family = "windows", case::long_running_process_shutdown_after_start(
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

        let started_supervisor = supervisor
            .start(sub_agent_internal_publisher)
            .expect("failed to start");

        if let Some(duration) = run_warmup_time {
            thread::sleep(duration)
        }

        let start = Instant::now();
        started_supervisor.stop().expect("failed to stop");
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

        thread::sleep(Duration::from_secs(1));
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

        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();
        let agent = agent
            .start(sub_agent_internal_publisher)
            .expect("failed start");

        thread::sleep(Duration::from_secs(2));
        agent.stop().expect("failed stop");

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

        thread::sleep(Duration::from_secs(1));

        logs_assert(|lines| {
            let count = lines
                .iter()
                .filter(|l| l.contains("Restarting supervisor"))
                .count();
            match count {
                3 => Ok(()),
                n => Err(format!(
                    "The supervisor should be restarted 3 times. Expected 3 lines, got {n}"
                )),
            }
        });
    }

    #[test]
    fn test_supervisor_health_events_on_breaking_backoff() {
        let backoff = Backoff::default()
            .with_initial_delay(Duration::new(0, 100))
            .with_max_retries(3)
            .with_last_retry_interval(Duration::new(30, 0));

        let exec_id = "echo-process";

        let executables = vec![
            #[cfg(target_family = "unix")]
            build_test_exec_data(r#"{"id":"echo-process","path":"echo","args":[]}"#)
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

        let executables_clone = agent.executables.clone();

        let executable_thread_contexts = executables_clone
            .iter()
            .map(|e| agent.start_process_thread(e, health_publisher.clone()));

        for thread_context in executable_thread_contexts {
            while !thread_context.is_thread_finished() {
                thread::sleep(Duration::from_millis(15));
            }
        }

        let start_time = SystemTime::now();

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

    #[test]
    fn test_supervisor_reloading_keeps_file_logging() {
        let dir = tempfile::tempdir().unwrap();
        let logging_path = dir.path().to_path_buf();

        let echo_cmd = if cfg!(windows) { "cmd" } else { "echo" };
        let unique_str_1 = "run1_unique_string";
        let args_1 = if cfg!(windows) {
            vec![
                "/C".to_string(),
                "echo".to_string(),
                unique_str_1.to_string(),
            ]
        } else {
            vec![unique_str_1.to_string()]
        };

        let exec_data_1 = ExecutableData {
            id: "echo-agent".to_string(),
            bin: echo_cmd.to_string(),
            args: args_1,
            env: HashMap::new(),
            shutdown_timeout: Duration::from_secs(5),
            restart_policy: RestartPolicy::default(),
        };

        let agent_identity = AgentIdentity::from((
            AgentID::try_from("test-agent".to_string()).unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let supervisor = NotStartedSupervisorOnHost::new(
            agent_identity.clone(),
            vec![exec_data_1],
            OnHostHealthConfig::default(),
            None,
            get_empty_packages(),
            Arc::new(MockPackageManager::new()),
        )
        .with_file_logging(true, logging_path.clone());

        let (pub_internal, _sub_internal) = pub_sub();
        let started_supervisor = supervisor
            .spin_up(pub_internal.clone())
            .expect("failed to start");

        // Wait a bit for the process to run and write logs
        std::thread::sleep(Duration::from_secs(2));

        let unique_str_2 = "run2_unique_string";
        let args_2 = if cfg!(windows) {
            vec![
                "/C".to_string(),
                "echo".to_string(),
                unique_str_2.to_string(),
            ]
        } else {
            vec![unique_str_2.to_string()]
        };

        let executable_rendered = Executable {
            id: "echo-agent".to_string(),
            path: echo_cmd.to_string(),
            args: Args(args_2),
            env: Env(HashMap::new()),
            restart_policy: RestartPolicyConfig::default(),
        };

        let on_host_config = OnHost {
            executables: vec![executable_rendered],
            enable_file_logging: true,
            health: OnHostHealthConfig::default(),
            version: None,
            filesystem: FileSystem::default(),
            packages: get_empty_packages(),
        };

        let runtime = Runtime {
            deployment: Deployment {
                linux: cfg!(unix).then(|| on_host_config.clone()),
                windows: cfg!(windows).then(|| on_host_config.clone()),
                k8s: None,
            },
        };

        let effective_agent = EffectiveAgent::new(agent_identity.clone(), runtime);

        let started_supervisor = started_supervisor
            .apply(effective_agent)
            .expect("failed to apply");

        // Wait a bit for the process to run and write logs
        std::thread::sleep(Duration::from_secs(2));

        started_supervisor.stop().expect("failed to stop");

        // Verify logs
        let agent_logs_dir = logging_path.join(agent_identity.id.to_string());
        assert!(
            agent_logs_dir.exists(),
            "Log directory {:?} does not exist",
            agent_logs_dir
        );

        let all_contents = fs::read_dir(agent_logs_dir)
            .expect("should find logs dir")
            .map(|entry| entry.expect("entry").path())
            .filter(|p| {
                // The `echo` commands should write to stdout, so we look for these files only.
                // Filtering by prefix because the timestamp is appended to the file name.
                p.file_prefix()
                    .is_some_and(|n| n.to_string_lossy().starts_with("stdout"))
            })
            .map(|p| fs::read_to_string(p).unwrap_or_default())
            // we just merge all contents
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            all_contents.contains(unique_str_1),
            "First run log not found (pre-apply)"
        );

        assert!(
            all_contents.contains(unique_str_2),
            "Second run log not found (post-apply)"
        );
    }

    #[test]
    fn test_supervisor_reloading_enables_file_logging() {
        let dir = tempfile::tempdir().unwrap();
        let logging_path = dir.path().to_path_buf();

        let echo_cmd = if cfg!(windows) { "cmd" } else { "echo" };
        let unique_str_1 = "run1_unique_string";
        let args_1 = if cfg!(windows) {
            vec![
                "/C".to_string(),
                "echo".to_string(),
                unique_str_1.to_string(),
            ]
        } else {
            vec![unique_str_1.to_string()]
        };

        let exec_data_1 = ExecutableData {
            id: "echo-agent".to_string(),
            bin: echo_cmd.to_string(),
            args: args_1,
            env: HashMap::new(),
            shutdown_timeout: Duration::from_secs(5),
            restart_policy: RestartPolicy::default(),
        };

        let agent_identity = AgentIdentity::from((
            AgentID::try_from("test-agent".to_string()).unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        // Start with logging DISABLED
        let supervisor = NotStartedSupervisorOnHost::new(
            agent_identity.clone(),
            vec![exec_data_1],
            OnHostHealthConfig::default(),
            None,
            get_empty_packages(),
            Arc::new(MockPackageManager::new()),
        )
        .with_file_logging(false, logging_path.clone());

        let (pub_internal, _sub_internal) = pub_sub();
        let started_supervisor = supervisor
            .spin_up(pub_internal.clone())
            .expect("failed to start");

        std::thread::sleep(Duration::from_secs(2));

        let unique_str_2 = "run2_unique_string";
        let args_2 = if cfg!(windows) {
            vec![
                "/C".to_string(),
                "echo".to_string(),
                unique_str_2.to_string(),
            ]
        } else {
            vec![unique_str_2.to_string()]
        };

        let executable_rendered = Executable {
            id: "echo-agent".to_string(),
            path: echo_cmd.to_string(),
            args: Args(args_2),
            env: Env(HashMap::new()),
            restart_policy: RestartPolicyConfig::default(),
        };

        // ENABLING file logging on reload
        let on_host_config = OnHost {
            executables: vec![executable_rendered],
            enable_file_logging: true,
            health: OnHostHealthConfig::default(),
            version: None,
            filesystem: FileSystem::default(),
            packages: get_empty_packages(),
        };

        let runtime = Runtime {
            deployment: Deployment {
                linux: cfg!(unix).then(|| on_host_config.clone()),
                windows: cfg!(windows).then(|| on_host_config.clone()),
                k8s: None,
            },
        };

        let effective_agent = EffectiveAgent::new(agent_identity.clone(), runtime);

        let started_supervisor = started_supervisor
            .apply(effective_agent)
            .expect("failed to apply");

        std::thread::sleep(Duration::from_secs(2));

        started_supervisor.stop().expect("failed to stop");

        let agent_logs_dir = logging_path.join(agent_identity.id.to_string());
        assert!(
            agent_logs_dir.exists(),
            "Log directory {:?} should exist",
            agent_logs_dir
        );

        let all_contents = fs::read_dir(agent_logs_dir)
            .expect("should find logs dir")
            .map(|entry| entry.expect("entry").path())
            .filter(|p| {
                // The `echo` commands should write to stdout, so we look for these files only.
                // Filtering by prefix because the timestamp is appended to the file name.
                p.file_prefix()
                    .is_some_and(|n| n.to_string_lossy().starts_with("stdout"))
            })
            .map(|p| fs::read_to_string(p).unwrap_or_default())
            // we just merge all contents
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            !all_contents.contains(unique_str_1),
            "First run log SHOULD NOT be found (it was disabled)"
        );

        assert!(
            all_contents.contains(unique_str_2),
            "Second run log SHOULD be found (it was enabled)"
        );
    }

    #[test]
    fn test_supervisor_reloading_disables_file_logging() {
        let dir = tempfile::tempdir().unwrap();
        let logging_path = dir.path().to_path_buf();

        let echo_cmd = if cfg!(windows) { "cmd" } else { "echo" };
        let unique_str_1 = "run1_unique_string";
        let args_1 = if cfg!(windows) {
            vec![
                "/C".to_string(),
                "echo".to_string(),
                unique_str_1.to_string(),
            ]
        } else {
            vec![unique_str_1.to_string()]
        };

        let exec_data_1 = ExecutableData {
            id: "echo-agent".to_string(),
            bin: echo_cmd.to_string(),
            args: args_1,
            env: HashMap::new(),
            shutdown_timeout: Duration::from_secs(5),
            restart_policy: RestartPolicy::default(),
        };

        let agent_identity = AgentIdentity::from((
            AgentID::try_from("test-agent".to_string()).unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        // Start with logging ENABLED
        let supervisor = NotStartedSupervisorOnHost::new(
            agent_identity.clone(),
            vec![exec_data_1],
            OnHostHealthConfig::default(),
            None,
            get_empty_packages(),
            Arc::new(MockPackageManager::new()),
        )
        .with_file_logging(true, logging_path.clone());

        let (pub_internal, _sub_internal) = pub_sub();
        let started_supervisor = supervisor
            .spin_up(pub_internal.clone())
            .expect("failed to start");

        std::thread::sleep(Duration::from_secs(2));

        let unique_str_2 = "run2_unique_string";
        let args_2 = if cfg!(windows) {
            vec![
                "/C".to_string(),
                "echo".to_string(),
                unique_str_2.to_string(),
            ]
        } else {
            vec![unique_str_2.to_string()]
        };

        let executable_rendered = Executable {
            id: "echo-agent".to_string(),
            path: echo_cmd.to_string(),
            args: Args(args_2),
            env: Env(HashMap::new()),
            restart_policy: RestartPolicyConfig::default(),
        };

        // DISABLING file logging on reload
        let on_host_config = OnHost {
            executables: vec![executable_rendered],
            enable_file_logging: false,
            health: OnHostHealthConfig::default(),
            version: None,
            filesystem: FileSystem::default(),
            packages: get_empty_packages(),
        };

        let runtime = Runtime {
            deployment: Deployment {
                linux: cfg!(unix).then(|| on_host_config.clone()),
                windows: cfg!(windows).then(|| on_host_config.clone()),
                k8s: None,
            },
        };

        let effective_agent = EffectiveAgent::new(agent_identity.clone(), runtime);

        let started_supervisor = started_supervisor
            .apply(effective_agent)
            .expect("failed to apply");

        std::thread::sleep(Duration::from_secs(2));

        started_supervisor.stop().expect("failed to stop");

        let agent_logs_dir = logging_path.join(agent_identity.id.to_string());
        assert!(
            agent_logs_dir.exists(),
            "Log directory {:?} should exist (from first run)",
            agent_logs_dir
        );

        let all_contents = fs::read_dir(agent_logs_dir)
            .expect("should find logs dir")
            .map(|entry| entry.expect("entry").path())
            .filter(|p| {
                // The `echo` commands should write to stdout, so we look for these files only.
                // Filtering by prefix because the timestamp is appended to the file name.
                p.file_prefix()
                    .is_some_and(|n| n.to_string_lossy().starts_with("stdout"))
            })
            .map(|p| fs::read_to_string(p).unwrap_or_default())
            // we just merge all contents to handle the corner case of multiple log files
            // e.g. hourly log rotation while the test is running
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            all_contents.contains(unique_str_1),
            "First run log SHOULD be found (it was enabled)"
        );

        assert!(
            !all_contents.contains(unique_str_2),
            "Second run log SHOULD NOT be found (it was disabled)"
        );
    }

    #[test]
    fn test_supervisor_reloading_keeps_file_logging_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let logging_path = dir.path().to_path_buf();

        let echo_cmd = if cfg!(windows) { "cmd" } else { "echo" };
        let unique_str_1 = "run1_unique_string";
        let args_1 = if cfg!(windows) {
            vec![
                "/C".to_string(),
                "echo".to_string(),
                unique_str_1.to_string(),
            ]
        } else {
            vec![unique_str_1.to_string()]
        };

        let exec_data_1 = ExecutableData {
            id: "echo-agent".to_string(),
            bin: echo_cmd.to_string(),
            args: args_1,
            env: HashMap::new(),
            shutdown_timeout: Duration::from_secs(5),
            restart_policy: RestartPolicy::default(),
        };

        let agent_identity = AgentIdentity::from((
            AgentID::try_from("test-agent".to_string()).unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        // Start with logging DISABLED
        let supervisor = NotStartedSupervisorOnHost::new(
            agent_identity.clone(),
            vec![exec_data_1],
            OnHostHealthConfig::default(),
            None,
            get_empty_packages(),
            Arc::new(MockPackageManager::new()),
        )
        .with_file_logging(false, logging_path.clone());

        let (pub_internal, _sub_internal) = pub_sub();
        let started_supervisor = supervisor
            .spin_up(pub_internal.clone())
            .expect("failed to start");

        std::thread::sleep(Duration::from_secs(2));

        let unique_str_2 = "run2_unique_string";
        let args_2 = if cfg!(windows) {
            vec![
                "/C".to_string(),
                "echo".to_string(),
                unique_str_2.to_string(),
            ]
        } else {
            vec![unique_str_2.to_string()]
        };

        let executable_rendered = Executable {
            id: "echo-agent".to_string(),
            path: echo_cmd.to_string(),
            args: Args(args_2),
            env: Env(HashMap::new()),
            restart_policy: RestartPolicyConfig::default(),
        };

        // KEEP logging DISABLED on reload
        let on_host_config = OnHost {
            executables: vec![executable_rendered],
            enable_file_logging: false,
            health: OnHostHealthConfig::default(),
            version: None,
            filesystem: FileSystem::default(),
            packages: get_empty_packages(),
        };

        let runtime = Runtime {
            deployment: Deployment {
                linux: cfg!(unix).then(|| on_host_config.clone()),
                windows: cfg!(windows).then(|| on_host_config.clone()),
                k8s: None,
            },
        };

        let effective_agent = EffectiveAgent::new(agent_identity.clone(), runtime);

        let started_supervisor = started_supervisor
            .apply(effective_agent)
            .expect("failed to apply");

        std::thread::sleep(Duration::from_secs(2));

        started_supervisor.stop().expect("failed to stop");

        let agent_logs_dir = logging_path.join(agent_identity.id.to_string());
        assert!(
            !agent_logs_dir.exists(),
            "Log directory {:?} should NOT exist",
            agent_logs_dir
        );
    }
}
