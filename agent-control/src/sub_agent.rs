pub mod collection;
pub mod effective_agents_assembler;
pub mod error;
pub(crate) mod event_handler;
pub mod health_checker;
pub mod identity;
pub mod k8s;
pub mod on_host;
pub mod remote_config_parser;
pub mod supervisor;
pub mod version;

use crate::agent_control::defaults::default_capabilities;
use crate::agent_control::run::Environment;
use crate::agent_control::uptime_report::{UptimeReportConfig, UptimeReporter};
use crate::event::SubAgentEvent::SubAgentStarted;
use crate::event::broadcaster::unbounded::UnboundedBroadcast;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::{OpAMPEvent, SubAgentEvent, SubAgentInternalEvent};
use crate::health::events::HealthEventPublisher;
use crate::health::health_checker::{Health, Unhealthy};
use crate::health::with_start_time::HealthWithStartTime;
use crate::opamp::operations::stop_opamp_client;
use crate::opamp::remote_config::OpampRemoteConfig;
use crate::opamp::remote_config::hash::{ConfigState, Hash};
use crate::opamp::remote_config::report::report_state;
use crate::utils::threads::spawn_named_thread;
use crate::values::config::{Config, RemoteConfig};
use crate::values::config_repository::ConfigRepository;
use crate::values::yaml_config::YAMLConfig;
use crossbeam::channel::never;
use crossbeam::select;
use effective_agents_assembler::EffectiveAgentsAssemblerError;
use effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssembler};
use error::SubAgentStopError;
use error::{SubAgentBuilderError, SubAgentError, SupervisorCreationError};
use event_handler::on_health::on_health;
use event_handler::on_version::on_version;
use identity::AgentIdentity;
use opamp_client::StartedClient;
use remote_config_parser::{RemoteConfigParser, RemoteConfigParserError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::SystemTime;
use supervisor::builder::SupervisorBuilder;
use supervisor::starter::{SupervisorStarter, SupervisorStarterError};
use supervisor::stopper::SupervisorStopper;
use tracing::{debug, error, info, info_span, trace, warn};

/// NotStartedSubAgent exposes a run method that starts processing events and, if present, the supervisor.
pub trait NotStartedSubAgent {
    type StartedSubAgent: StartedSubAgent;
    /// The run method (non-blocking) starts processing events and, if present, the supervisor.
    /// It returns a StartedSubAgent exposing .stop() to manage the running process.
    fn run(self) -> Self::StartedSubAgent;
}

/// The StartedSubAgent trait defines the interface for a supervisor that is already running.
///
/// Exposes information about the Sub Agent and a stop method that will stop the
/// supervised processes' execution and the loop processing the events.
pub trait StartedSubAgent {
    /// Stops all internal services owned by the SubAgent
    fn stop(self) -> Result<(), SubAgentStopError>;
}

pub trait SubAgentBuilder {
    type NotStartedSubAgent: NotStartedSubAgent;
    fn build(
        &self,
        agent_identity: &AgentIdentity,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError>;
}

type BuilderSupervisorStopper<B> =
    <<B as SupervisorBuilder>::SupervisorStarter as SupervisorStarter>::SupervisorStopper;

/// SubAgentStopper is implementing the StartedSubAgent trait.
///
/// It stores the runtime JoinHandle and a SubAgentInternalEvent publisher.
/// It's stored in the agent-control's NotStartedSubAgents collection to be able to call
/// the exposed method Stop that will publish a StopRequested event to the runtime
/// and wait on the JoinHandle for the runtime to finish.
pub struct SubAgentStopper {
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    runtime: JoinHandle<Result<(), SubAgentError>>,
}

/// SubAgent is implementing the NotStartedSubAgent trait so only the method run
/// can be called from the AgentControl to start the runtime and receive a StartedSubAgent
/// that can be stopped
///
/// All its methods are internal and only called from the runtime method that spawns
/// a thread listening to events and acting on them.
pub struct SubAgent<C, B, R, Y, A>
where
    C: StartedClient + Send + Sync + 'static,
    B: SupervisorBuilder + Send + Sync + 'static,
    R: RemoteConfigParser + Send + Sync + 'static,
    Y: ConfigRepository + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
{
    pub(super) identity: AgentIdentity,
    pub(super) maybe_opamp_client: Option<C>,
    pub(super) sub_agent_publisher: UnboundedBroadcast<SubAgentEvent>,
    pub(super) sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
    pub(super) sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
    pub(super) sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    remote_config_parser: Arc<R>,
    supervisor_builder: Arc<B>,
    config_repository: Arc<Y>,
    effective_agent_assembler: Arc<A>,
    environment: Environment,
}

impl<C, B, R, Y, A> SubAgent<C, B, R, Y, A>
where
    C: StartedClient + Send + Sync + 'static,
    B: SupervisorBuilder + Send + Sync + 'static,
    R: RemoteConfigParser + Send + Sync + 'static,
    Y: ConfigRepository + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        identity: AgentIdentity,
        maybe_opamp_client: Option<C>,
        supervisor_builder: Arc<B>,
        sub_agent_publisher: UnboundedBroadcast<SubAgentEvent>,
        sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
        (sub_agent_internal_publisher, sub_agent_internal_consumer): (
            EventPublisher<SubAgentInternalEvent>,
            EventConsumer<SubAgentInternalEvent>,
        ),
        remote_config_parser: Arc<R>,
        config_repository: Arc<Y>,
        effective_agent_assembler: Arc<A>,
        environment: Environment,
    ) -> Self {
        Self {
            identity,
            maybe_opamp_client,
            supervisor_builder,
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_publisher,
            sub_agent_internal_consumer,
            remote_config_parser,
            config_repository,
            effective_agent_assembler,
            environment,
        }
    }

    /// Attempt to build a supervisor specific for this sub-agent given an existing YAML config.
    ///
    /// This function retrieves the stored remote config hash (if any) for this sub-agent identity,
    /// though it does not cancel the operation if the hash is failed as we can still have a valid configuration (either
    /// a previous valid remote configuration or a local configuration).
    ///
    /// Any failure to assemble the effective agent or the supervisor, or failure to start the
    /// supervisor will be mark the existing hash as failed and report the error if there's an
    /// OpAMP client present in the sub-agent.
    fn init_supervisor(&self) -> Option<BuilderSupervisorStopper<B>> {
        // An earlier run of Agent Control might have data for this agent identity, so we
        // attempt to retrieve an existing remote config,
        // falling back to a local config if there's no remote config.
        // If there's no config at all, we cannot assemble a supervisor, so we just return immediately.
        let Some(config) = self
            .config_repository
            .load_remote_fallback_local(&self.identity.id, &default_capabilities())
            .inspect_err(|e| {
                warn!(error = %e, "Failed to load remote or local configuration");
            })
            .ok()
            .flatten()
        else {
            debug!("No configuration found for sub-agent");
            // The effective config needs to be reported with the local config that failed
            // to start the supervisor (not ideal but better than leaving the deleted remote),
            // if not FC could still consider the previous remote that has just been deleted.
            self.maybe_opamp_client.as_ref().inspect(|c| {
                let _ = c
                    .update_effective_config()
                    .inspect_err(|e| error!("Effective config update failed: {e}"));
            });
            return None;
        };

        let effective_agent = self
            .effective_agent(config.get_yaml_config().clone())
            .map_err(SupervisorCreationError::from);

        let not_started_supervisor = effective_agent.and_then(|effective_agent| {
            self.supervisor_builder
                .build_supervisor(effective_agent)
                .map_err(SupervisorCreationError::from)
        });

        if not_started_supervisor.is_ok() {
            // Communicate the config that we will be using
            // FIXME: only if we successfully build a supervisor?
            // What if we fail and we don't have a supervisor? Should we report?

            // During the sub-agent runtime we need to persist the configuration before updating the
            // effective_config (the callback reads from the storage) but, since the configuration
            // is already in storage because we are just starting the agent for the first time, and
            // we retrieved the information we work with in this function from the storage, we don't
            // need to perform any storing at this point, the data was already present there.
            self.maybe_opamp_client.as_ref().inspect(|c| {
                let _ = c
                    .update_effective_config()
                    .inspect_err(|e| error!("Effective config update failed: {e}"));
            });
        }

        let started_supervisor = not_started_supervisor.and_then(|stopped_supervisor| {
            self.start_supervisor(stopped_supervisor)
                .map_err(SupervisorCreationError::from)
        });

        // After all operations, set the hash to a final state
        // only if it was in the `applying` state.
        if let Config::RemoteConfig(remote_config) = config {
            if remote_config.is_applying() {
                let state = match &started_supervisor {
                    Ok(_) => ConfigState::Applied,
                    Err(e) => ConfigState::Failed {
                        error_message: e.to_string(),
                    },
                };
                let remote_config = remote_config.with_state(state);

                if let Some(opamp_client) = &self.maybe_opamp_client {
                    let _ = report_state(
                        remote_config.state.clone(),
                        remote_config.hash,
                        opamp_client,
                    );
                }
                // As the hash might have changed state from the above operations, we store it
                self.update_remote_config_state(remote_config.state);
            }
        }

        started_supervisor.ok()
    }

    pub fn runtime(self) -> JoinHandle<Result<(), SubAgentError>> {
        spawn_named_thread("Subagent runtime", move || {
            let span = info_span!("start_agent", id=%self.identity.id);
            let _span_guard = span.enter();

            let mut supervisor = self.init_supervisor();

            // Stores the current health state for logging purposes.
            let mut previous_health = None;

            debug!("runtime started");
            self.sub_agent_publisher
                .broadcast(SubAgentStarted(self.identity.clone(), SystemTime::now()));

            // The below two lines are used to create a channel that never receives any message
            // if the sub_agent_opamp_consumer is None. Thus, we avoid erroring if there is no
            // publisher for OpAMP events, and we attempt to receive them, as erroring while reading
            // from this channel will break the loop and prevent the reception of sub-agent
            // internal events if OpAMP is globally disabled in the agent-control config.
            let never_receive = EventConsumer::from(never());
            let opamp_receiver = self
                .sub_agent_opamp_consumer
                .as_ref()
                .unwrap_or(&never_receive);
            // TODO: We should separate the loop for OpAMP events and internal events into two
            // different loops, which currently is not straight forward due to sharing structures
            // that need to be moved into thread closures.

            // Report uptime every 60 seconds
            let uptime_report_config = &UptimeReportConfig::default();
            let uptime_reporter = UptimeReporter::from(uptime_report_config);
            // If a uptime report is configured, we trace it for the first time here
            if uptime_report_config.enabled() {
                let _ = uptime_reporter.report();
            }

            drop(_span_guard);

            // Count the received remote configs during execution
            let mut remote_config_count = 0;
            loop {
                select! {
                    recv(opamp_receiver.as_ref()) -> opamp_event_res => {
                        let span = info_span!("process_fleet_event", id=%self.identity.id);
                        let _span_guard = span.enter();
                        match opamp_event_res {
                            Err(e) => {
                                debug!(error = %e, select_arm = "sub_agent_opamp_consumer", "Channel closed");
                                break;
                            },
                            Ok(OpAMPEvent::RemoteConfigReceived(config)) => {
                                debug!(
                                    select_arm = "sub_agent_opamp_consumer",
                                    "Remote config received"
                                );
                                // This branch only makes sense with a valid OpAMP client
                                let Some(opamp_client) = &self.maybe_opamp_client else {
                                    debug!("Got remote config without OpAMP being enabled");
                                    continue;
                                };
                                // Trace the occurrence of a remote config reception
                                remote_config_count += 1;
                                trace!(monotonic_counter.remote_configs_received = remote_config_count);

                                // Refresh the supervisor according to the received config
                                supervisor = self.handle_remote_config(opamp_client, config, supervisor);
                            },
                            Ok(OpAMPEvent::Connected) | Ok(OpAMPEvent::ConnectFailed(_, _)) => {},
                        }
                    },
                    recv(&self.sub_agent_internal_consumer.as_ref()) -> sub_agent_internal_event_res => {
                        let span = info_span!("process_event", id=%self.identity.id);
                        let _span_guard = span.enter();
                        match sub_agent_internal_event_res {
                            Err(e) => {
                                debug!(error = %e, select_arm = "sub_agent_internal_consumer", "Channel closed");
                                break;
                            },
                            Ok(SubAgentInternalEvent::StopRequested) => {
                                debug!(select_arm = "sub_agent_internal_consumer", "StopRequested");
                                stop_supervisor(supervisor);
                                break;
                            },
                            Ok(SubAgentInternalEvent::AgentHealthInfo(health))=>{
                                debug!(select_arm = "sub_agent_internal_consumer", ?health, "AgentHealthInfo");

                                let health_state = Health::from(health.clone());
                                if !is_health_state_equal_to_previous_state(&previous_health, &health_state) {
                                    log_health_info(&health_state);
                                }
                                previous_health = Some(health_state);
                                let _ = on_health(
                                    health,
                                    self.maybe_opamp_client.as_ref(),
                                    self.sub_agent_publisher.clone(),
                                    self.identity.clone(),
                                )
                                .inspect_err(|e| error!(error = %e, select_arm = "sub_agent_internal_consumer", "Processing health message"));
                            },
                            Ok(SubAgentInternalEvent::AgentVersionInfo(agent_data)) => {
                                let _ = on_version(
                                    agent_data,
                                    self.maybe_opamp_client.as_ref(),
                                    )
                                    .inspect_err(|e| error!(error = %e, select_arm = "sub_agent_internal_consumer", "processing version message"));
                            }
                        }
                    }
                    recv(uptime_reporter.receiver()) -> _tick => { let _ = uptime_reporter.report(); },
                }
            }

            stop_opamp_client(self.maybe_opamp_client, &self.identity.id)
        })
    }

    /// This function handles the remote config received from OpAMP.
    ///
    /// Besides the config itself, it receives the old supervisor so we can operate over it
    /// depending on the outcome of the build attempt of a new supervisor using the provided config:
    ///
    ///   - If the build is successful, the old supervisor is stopped and the new one is returned.
    ///   - If the build fails, the old supervisor is not stopped and the new one is not returned.
    ///   - A specific case is when the received remote config comes specifically empty, in which
    ///     case we intentionally stop the supervisor and leave the runtime without it, waiting for
    ///     a new incoming remote config which will call this function again.
    fn handle_remote_config(
        &self,
        opamp_client: &C,
        config: OpampRemoteConfig,
        old_supervisor: Option<BuilderSupervisorStopper<B>>,
    ) -> Option<BuilderSupervisorStopper<B>> {
        // If hash is same as the stored and is not on status applying (processing was incomplete),
        // the previous working supervisor will keep running but the status will be reported again.
        if let Ok(Some(rc)) = self.config_repository.get_remote_config(&self.identity.id) {
            if config.hash == rc.hash && !rc.state.is_applying() {
                let _ = report_state(rc.state, rc.hash, opamp_client);
                return old_supervisor;
            }
        }

        // If the remote hash comes failed from the pre-processing steps (performed in the OpAMP
        // client callbacks, see `process_remote_config` in `opamp::callbacks`),
        // the previous working supervisor will keep running and the hash won't be updated.
        if Self::check_and_report_config_failed(opamp_client, &config) {
            return old_supervisor;
        }

        info!(hash = config.hash.to_string(), "Applying remote config");
        let _ = report_state(ConfigState::Applying, config.hash.clone(), opamp_client);

        // Start transforming the remote config
        // Attempt to parse/validate the remote config
        let parsed_remote = self
            .remote_config_parser
            .parse(self.identity.clone(), &config);

        let not_started_supervisor = match parsed_remote.clone() {
            Ok(remote_config) => {
                // If parsing was successful, call the function with Some(remote_config)
                self.create_supervisor_from_remote_config(&remote_config)
            }
            Err(error) => {
                warn!("Failed to parse remote configuration: {}", error);

                Err(error.into())
            }
        };

        // Now, we should have either a Supervisor or an error to handle later,
        // which can come from either:
        //   - a parse failure
        //   - having empty values
        //   - the EffectiveAgent assembly attempt
        //   - the Supervisor assembly attempt
        // We report the state and effective config and return a supervisor if it can be started or reused
        match not_started_supervisor {
            // If all correct, return new supervisor
            Ok(new_supervisor) => self.start_new_supervisor_reporting_config_and_state(
                opamp_client,
                &config.hash,
                old_supervisor,
                new_supervisor,
            ),
            // If we have no configuration, stop the old supervisor and return None.
            Err(SupervisorCreationError::NoConfiguration) => {
                // Stop old supervisor if any
                stop_supervisor(old_supervisor);

                // Report the config as applied
                let _ = report_state(ConfigState::Applied, config.hash, opamp_client);

                // The effective config needs to be reported with the empty config.
                let _ = opamp_client
                    .update_effective_config()
                    .inspect_err(|e| error!("Effective config update failed: {e}"));
                None
            }
            Err(e) => {
                warn!("Failed to build supervisor: {e}");

                // If the remote config was deleted but creating the supervisor from local failed
                // stop the old supervisor and return None.
                if Self::check_and_report_local_failed(opamp_client, &config.hash, parsed_remote) {
                    stop_supervisor(old_supervisor);
                    return None;
                }

                let _ = report_state(
                    ConfigState::Failed {
                        error_message: e.to_string(),
                    },
                    config.hash,
                    opamp_client,
                );

                // If we fail to build the supervisor, we don't stop the old one and return it back
                old_supervisor
            }
        }
    }

    fn check_and_report_local_failed(
        opamp_client: &C,
        hash: &Hash,
        parsed_remote: Result<Option<RemoteConfig>, RemoteConfigParserError>,
    ) -> bool {
        if let Ok(None) = parsed_remote {
            // Report the empty remote config as applied
            let _ = report_state(ConfigState::Applied, hash.clone(), opamp_client);

            // The effective config needs to be reported with the local config that failed
            // to start the supervisor (not ideal but better than leaving the deleted remote),
            // if not FC could still consider the previous remote that has just been deleted.
            let _ = opamp_client
                .update_effective_config()
                .inspect_err(|e| error!("Effective config update failed: {e}"));

            return true;
        }
        false
    }

    fn start_new_supervisor_reporting_config_and_state(
        &self,
        opamp_client: &C,
        hash: &Hash,
        old_supervisor: Option<BuilderSupervisorStopper<B>>,
        new_supervisor: <B as SupervisorBuilder>::SupervisorStarter,
    ) -> Option<BuilderSupervisorStopper<B>> {
        let _ = opamp_client
            .update_effective_config()
            .inspect_err(|e| error!("Effective config update failed: {e}"));

        // Stop old supervisor if any. This needs to happen before starting the new one
        stop_supervisor(old_supervisor);

        // Start the new supervisor
        self.start_supervisor(new_supervisor)
            // Alter the state depending on the outcome
            .inspect(|_| {
                self.update_remote_config_state(ConfigState::Applied);
                // Report the empty remote config as applied
                let _ = report_state(ConfigState::Applied, hash.clone(), opamp_client);
            })
            .inspect_err(|e| {
                let state = ConfigState::Failed {
                    error_message: e.to_string(),
                };
                self.update_remote_config_state(state.clone());
                // Report the empty remote config as applied
                let _ = report_state(ConfigState::Applied, hash.clone(), opamp_client);
            })
            // Return it
            .ok()
    }

    // check_and_report_config_failed returns true if the config is failed and reports that state
    fn check_and_report_config_failed(opamp_client: &C, config: &OpampRemoteConfig) -> bool {
        if let Some(error_message) = config.state.error_message().cloned() {
            warn!(
                hash = %config.hash,
                "Remote configuration cannot be applied: {error_message}"
            );
            // Failed configurations are reported but not persisted.
            let _ = report_state(
                ConfigState::Failed { error_message },
                config.hash.clone(),
                opamp_client,
            );

            return true;
        }
        false
    }

    /// Parses incoming remote config, assembles and builds the supervisor.
    fn create_supervisor_from_remote_config(
        &self,
        parsed_remote: &Option<RemoteConfig>,
    ) -> Result<<B as SupervisorBuilder>::SupervisorStarter, SupervisorCreationError> {
        match parsed_remote {
            // Apply the remote config:
            // - Build supervisor
            // - Store if remote if build was successful
            Some(remote_config) => {
                let effective_agent = self.effective_agent(remote_config.config.clone())?;

                self.supervisor_builder
                    .build_supervisor(effective_agent)
                    .inspect(|_| {
                        let _ = self
                            .config_repository
                            .store_remote(&self.identity.id, remote_config)
                            .inspect_err(|e| {
                                warn!("Failed to store remote configuration: {e}");
                            });
                    })
            }
            // Reset to local config:
            // - Removes remote config
            // - Build supervisor from local config if exists
            None => {
                let _ = self
                    .config_repository
                    .delete_remote(&self.identity.id)
                    .inspect_err(|e| warn!("Failed to delete remote configuration: {e}"));

                let remote_config = self
                    .config_repository
                    .load_local(&self.identity.id)
                    .inspect_err(|e| warn!("Failed to load local configuration: {e}"))
                    .unwrap_or_default()
                    .ok_or(SupervisorCreationError::NoConfiguration)?;

                let effective_agent =
                    self.effective_agent(remote_config.get_yaml_config().clone())?;

                self.supervisor_builder.build_supervisor(effective_agent)
            }
        }
        .map_err(SupervisorCreationError::from)
    }

    pub(crate) fn start_supervisor(
        &self,
        not_started_supervisor: B::SupervisorStarter,
    ) -> Result<
        <B::SupervisorStarter as SupervisorStarter>::SupervisorStopper,
        SupervisorStarterError,
    > {
        not_started_supervisor
            .start(self.sub_agent_internal_publisher.clone())
            .inspect_err(|err| {
                let unhealthy = HealthWithStartTime::from_unhealthy(
                    Unhealthy::new(err.to_string()),
                    SystemTime::now(),
                );
                error!("Failure starting supervisor: {err}");
                self.sub_agent_internal_publisher
                    .publish_health_event(unhealthy);
            })
    }

    fn effective_agent(
        &self,
        yaml_config: YAMLConfig,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError> {
        // Assemble the new agent
        self.effective_agent_assembler.assemble_agent(
            &self.identity,
            yaml_config,
            &self.environment,
        )
    }

    fn update_remote_config_state(&self, config_state: ConfigState) {
        let _ = self
            .config_repository
            .update_state(&self.identity.id, config_state)
            .inspect_err(|err| {
                warn!("Could not update the config state: {err}");
            });
    }
}

fn is_health_state_equal_to_previous_state(
    previous_state: &Option<Health>,
    current_state: &Health,
) -> bool {
    match (previous_state, current_state) {
        (Some(Health::Healthy(_)), Health::Healthy(_)) => true,
        (Some(prev), current) => prev == current,
        _ => false,
    }
}

fn log_health_info(health: &Health) {
    match health {
        // From unhealthy (or initial) to healthy
        Health::Healthy(_) => {
            info!("Agent is healthy");
        }
        // Every time health is unhealthy
        Health::Unhealthy(unhealthy) => {
            warn!(
                status = unhealthy.status(),
                last_error = unhealthy.last_error(),
                "Agent is unhealthy"
            );
        }
    }
}

impl StartedSubAgent for SubAgentStopper {
    fn stop(self) -> Result<(), SubAgentStopError> {
        // Stop processing events
        self.sub_agent_internal_publisher
            .publish(SubAgentInternalEvent::StopRequested)?;
        // Wait for the sub agent thread to finish
        let runtime_join_result = self.runtime.join().map_err(|_| {
            // Error when the 'runtime thread' panics.
            SubAgentStopError::SubAgentJoinHandle(
                "The sub agent thread failed unexpectedly".to_string(),
            )
        })?;
        Ok(runtime_join_result?)
    }
}

pub fn stop_supervisor<S>(maybe_started_supervisor: Option<S>)
where
    S: SupervisorStopper,
{
    if let Some(s) = maybe_started_supervisor {
        let _ = s.stop().inspect_err(|err| {
            error!(%err,"Error stopping supervisor");
        });
    }
}

impl<C, B, R, Y, A> NotStartedSubAgent for SubAgent<C, B, R, Y, A>
where
    C: StartedClient + Send + Sync + 'static,
    B: SupervisorBuilder + Send + Sync + 'static,
    R: RemoteConfigParser + Send + Sync + 'static,
    Y: ConfigRepository + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
{
    type StartedSubAgent = SubAgentStopper;

    fn run(self) -> Self::StartedSubAgent {
        let sub_agent_internal_publisher = self.sub_agent_internal_publisher.clone();
        let runtime_handle = self.runtime();

        SubAgentStopper {
            sub_agent_internal_publisher,
            runtime: runtime_handle,
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    use super::effective_agents_assembler::LocalEffectiveAgentsAssembler;
    use super::remote_config_parser::AgentRemoteConfigParser;
    use super::supervisor::builder::tests::MockSupervisorBuilder;
    use super::supervisor::starter::tests::MockSupervisorStarter;
    use super::supervisor::stopper::tests::MockSupervisorStopper;
    use super::{NotStartedSubAgent, StartedSubAgent};
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_type::definition::AgentTypeDefinition;
    use crate::agent_type::embedded_registry::EmbeddedRegistry;
    use crate::agent_type::render::persister::config_persister_file::ConfigurationPersisterFile;
    use crate::agent_type::render::renderer::TemplateRenderer;
    use crate::event::channel::pub_sub;
    use crate::health::health_checker::{Healthy, Unhealthy};
    use crate::opamp::client_builder::tests::MockStartedOpAMPClient;
    use crate::opamp::remote_config::hash::Hash;
    use crate::opamp::remote_config::validators::tests::MockRemoteConfigValidator;
    use crate::opamp::remote_config::{ConfigurationMap, OpampRemoteConfig};
    use crate::values::config::RemoteConfig;
    use crate::values::config_repository::tests::InMemoryConfigRepository;
    use mockall::mock;
    use opamp_client::opamp::proto::{RemoteConfigStatus, RemoteConfigStatuses};
    use opamp_client::operation::capabilities::Capabilities;
    use rstest::*;
    use std::collections::HashMap;
    use std::ops::Deref;
    use std::sync::Arc;

    type TestSubAgent = SubAgent<
        MockStartedOpAMPClient,
        MockSupervisorBuilder<MockSupervisorStarter>,
        AgentRemoteConfigParser<MockRemoteConfigValidator>,
        InMemoryConfigRepository,
        LocalEffectiveAgentsAssembler<
            EmbeddedRegistry,
            TemplateRenderer<ConfigurationPersisterFile>,
        >,
    >;

    mock! {
        pub StartedSubAgent {}

        impl StartedSubAgent for StartedSubAgent {
            fn stop(self) -> Result<(), SubAgentStopError>;
        }
    }

    impl MockStartedSubAgent {
        pub fn should_stop(&mut self) {
            self.expect_stop().once().returning(|| Ok(()));
        }
    }

    mock! {
        pub NotStartedSubAgent {}

        impl NotStartedSubAgent for NotStartedSubAgent {
            type StartedSubAgent = MockStartedSubAgent;

            fn run(self) -> <Self as NotStartedSubAgent>::StartedSubAgent;
        }
    }

    impl MockNotStartedSubAgent {
        pub fn should_run(&mut self, started_sub_agent: MockStartedSubAgent) {
            self.expect_run()
                .once()
                .return_once(move || started_sub_agent);
        }
    }

    mock! {
        pub SubAgentBuilder {}

        impl SubAgentBuilder for SubAgentBuilder {
            type NotStartedSubAgent = MockNotStartedSubAgent;

            fn build(
                &self,
                agent_identity: &AgentIdentity,
            ) -> Result<<Self as SubAgentBuilder>::NotStartedSubAgent,  SubAgentBuilderError>;
        }
    }

    impl MockSubAgentBuilder {}

    #[rstest]
    #[case::healthy_states_same_status(Some(healthy("status")), healthy("status"))]
    #[case::healthy_states_different_status(Some(healthy("status a")), healthy("status b"))]
    #[case::unhealthy_states_with_same_content(
        Some(unhealthy("status", "error")),
        unhealthy("status", "error")
    )]
    fn test_health_state_is_equal_to_previous_state(
        #[case] previous_state: Option<Health>,
        #[case] current_state: Health,
    ) {
        assert!(is_health_state_equal_to_previous_state(
            &previous_state,
            &current_state
        ));
    }

    #[rstest]
    #[case::first_state_is_healthy(None, healthy("status"))]
    #[case::first_state_is_unhealthy(None, unhealthy("status", "error"))]
    #[case::healthy_and_unhealthy(Some(healthy("status")), unhealthy("status", "error"))]
    #[case::unhealthy_and_healthy(Some(unhealthy("status", "error")), healthy("status"))]
    #[case::two_unhealthy_states_with_different_status(
        Some(unhealthy("status a", "error")),
        unhealthy("status b", "error")
    )]
    #[case::two_unhealthy_states_with_different_errors(
        Some(unhealthy("status", "error a")),
        unhealthy("status", "error b")
    )]
    fn test_health_state_is_different_to_previous_state(
        #[case] previous_state: Option<Health>,
        #[case] current_state: Health,
    ) {
        assert!(!is_health_state_equal_to_previous_state(
            &previous_state,
            &current_state
        ));
    }

    fn healthy(status: &str) -> Health {
        Health::Healthy(Healthy::new().with_status(status.to_string()))
    }

    fn unhealthy(status: &str, error: &str) -> Health {
        Health::Unhealthy(Unhealthy::new(error.to_string()).with_status(status.to_string()))
    }

    /// Helpers for testing the config scenarios which some of the data their produce are related to each other.
    struct TestAgent;
    impl TestAgent {
        fn identity() -> AgentIdentity {
            AgentIdentity::default()
        }

        fn id() -> AgentID {
            AgentIdentity::default().id
        }

        fn agent_type_definition() -> AgentTypeDefinition {
            serde_yaml::from_str(
                r#"
name: default
namespace: default
version: 0.0.1
variables:
  common:
    var:
      description: "fake"
      type: string
      required: false
      default: ""
deployment:
  on_host:
    executable:
      path: ${nr-var:var}
"#,
            )
            .unwrap()
        }

        fn hash() -> Hash {
            Hash::from("hash")
        }

        fn status_applied() -> RemoteConfigStatus {
            RemoteConfigStatus {
                status: RemoteConfigStatuses::Applied as i32,
                last_remote_config_hash: Self::hash().to_string().into_bytes(),
                ..Default::default()
            }
        }

        fn status_applying() -> RemoteConfigStatus {
            RemoteConfigStatus {
                status: RemoteConfigStatuses::Applying as i32,
                last_remote_config_hash: Self::hash().to_string().into_bytes(),
                ..Default::default()
            }
        }

        fn status_failed() -> RemoteConfigStatus {
            RemoteConfigStatus {
                status: RemoteConfigStatuses::Failed as i32,
                last_remote_config_hash: Self::hash().to_string().into_bytes(),
                error_message: "could not build the supervisor from an effective agent: ``no configuration found``".into(),
            }
        }

        fn valid_config_yaml() -> YAMLConfig {
            "var: valid".try_into().unwrap()
        }

        fn valid_config_value() -> String {
            "valid".to_string()
        }

        fn valid_remote_config() -> OpampRemoteConfig {
            OpampRemoteConfig::new(
                Self::id(),
                Self::hash(),
                ConfigState::Applying,
                Some(ConfigurationMap::new(HashMap::from([(
                    "".to_string(),
                    Self::valid_config_yaml().try_into().unwrap(),
                )]))),
            )
        }

        fn reset_remote_config() -> OpampRemoteConfig {
            OpampRemoteConfig::new(
                Self::id(),
                Self::hash(),
                ConfigState::Applying,
                Some(ConfigurationMap::new(HashMap::from([(
                    "".to_string(),
                    // Reset signal
                    "".to_string(),
                )]))),
            )
        }

        fn failed_remote_config() -> OpampRemoteConfig {
            OpampRemoteConfig::new(
                Self::id(),
                Hash::from("failed hash"),
                ConfigState::Failed {
                    error_message: "error_message".to_string(),
                },
                Some(ConfigurationMap::new(HashMap::from([(
                    "".to_string(),
                    "broken config:".to_string(),
                )]))),
            )
        }
    }

    fn sub_agent(
        opamp_client: Option<MockStartedOpAMPClient>,
        supervisor_builder: MockSupervisorBuilder<MockSupervisorStarter>,
        config_repository: Arc<InMemoryConfigRepository>,
    ) -> TestSubAgent {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (_sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();

        let effective_agents_assembler = Arc::new(LocalEffectiveAgentsAssembler::new(
            Arc::new(TestAgent::agent_type_definition().into()),
            TemplateRenderer::default(),
        ));

        SubAgent::new(
            TestAgent::identity(),
            opamp_client,
            Arc::new(supervisor_builder),
            UnboundedBroadcast::default(),
            Some(sub_agent_opamp_consumer),
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(AgentRemoteConfigParser::<MockRemoteConfigValidator>::new(
                vec![],
            )),
            config_repository,
            effective_agents_assembler,
            Environment::OnHost,
        )
    }

    fn expect_supervisor_shut_down() -> MockSupervisorStopper {
        let mut supervisor = MockSupervisorStopper::new();
        supervisor.should_stop();
        supervisor
    }
    fn expect_supervisor_does_not_stop() -> MockSupervisorStopper {
        let mut supervisor = MockSupervisorStopper::new();
        supervisor.expect_stop().never();
        supervisor
    }
    fn expect_fail_to_build_supervisor() -> MockSupervisorBuilder<MockSupervisorStarter> {
        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .once()
            .return_once(|_| Err(SubAgentError::NoConfiguration.into()));
        supervisor_builder
    }
    fn expect_supervisor_do_not_build() -> MockSupervisorBuilder<MockSupervisorStarter> {
        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder.expect_build_supervisor().never();
        supervisor_builder
    }
    fn expect_build_supervisor_with(
        expected_config_value: String,
    ) -> MockSupervisorBuilder<MockSupervisorStarter> {
        let mut supervisor_builder = MockSupervisorBuilder::new();
        let started_supervisor = MockSupervisorStopper::new();
        let mut stopped_supervisor = MockSupervisorStarter::new();
        stopped_supervisor.should_start(started_supervisor);
        supervisor_builder
            .expect_build_supervisor()
            .once()
            .withf(move |effective_agent| {
                effective_agent
                    .get_onhost_config()
                    .unwrap()
                    .executable
                    .as_ref()
                    .unwrap()
                    .path
                    .clone()
                    .get()
                    .eq(&expected_config_value.clone())
            })
            .return_once(|_| Ok(stopped_supervisor));
        supervisor_builder
    }
    fn test_mocks() -> (Arc<InMemoryConfigRepository>, MockStartedOpAMPClient) {
        let config_repository = Arc::new(InMemoryConfigRepository::default());
        let opamp_client = MockStartedOpAMPClient::new();
        (config_repository, opamp_client)
    }

    #[test]
    fn test_gracefully_stop_empty_sub_agent() {
        let (config_repository, _opamp_client) = test_mocks();

        let supervisor_builder = expect_supervisor_do_not_build();

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();

        let effective_agents_assembler = Arc::new(LocalEffectiveAgentsAssembler::new(
            Arc::new(TestAgent::agent_type_definition().into()),
            TemplateRenderer::default(),
        ));

        let sub_agent = SubAgent::new(
            TestAgent::identity(),
            None::<MockStartedOpAMPClient>,
            Arc::new(supervisor_builder),
            UnboundedBroadcast::default(),
            None,
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(AgentRemoteConfigParser::<MockRemoteConfigValidator>::new(
                vec![],
            )),
            config_repository,
            effective_agents_assembler,
            Environment::OnHost,
        );

        sub_agent.run().stop().unwrap();
    }
    #[test]
    fn test_remote_config_applying_to_applied() {
        let (config_repository, mut opamp_client) = test_mocks();

        let supervisor_builder = expect_build_supervisor_with(TestAgent::valid_config_value());
        opamp_client.should_update_effective_config(1);
        opamp_client.should_set_remote_config_status_seq(vec![
            TestAgent::status_applying(),
            TestAgent::status_applied(),
        ]);

        let sub_agent = sub_agent(
            Some(opamp_client),
            supervisor_builder,
            config_repository.clone(),
        );

        let old_supervisor = Some(expect_supervisor_shut_down());

        let new_supervisor = sub_agent.handle_remote_config(
            sub_agent.maybe_opamp_client.as_ref().unwrap(),
            TestAgent::valid_remote_config(),
            old_supervisor,
        );

        assert_remote_config(
            config_repository.deref(),
            &TestAgent::id(),
            |remote_config| {
                assert_eq!(
                    remote_config.hash.to_string(),
                    TestAgent::hash().to_string()
                );
                assert!(remote_config.state.is_applied());
            },
        );

        assert_eq!(
            config_repository
                .load_remote(&TestAgent::id(), &Capabilities::default())
                .unwrap()
                .unwrap()
                .get_yaml_config()
                .clone(),
            TestAgent::valid_config_yaml()
        );

        assert!(new_supervisor.is_some());
    }
    #[test]
    fn test_remote_config_applying_to_failed() {
        let (config_repository, mut opamp_client) = test_mocks();

        let supervisor_builder = expect_fail_to_build_supervisor();
        opamp_client.should_set_remote_config_status_seq(vec![
            TestAgent::status_applying(),
            TestAgent::status_failed(),
        ]);

        let sub_agent = sub_agent(
            Some(opamp_client),
            supervisor_builder,
            config_repository.clone(),
        );

        let old_supervisor = Some(expect_supervisor_does_not_stop());

        let new_supervisor = sub_agent.handle_remote_config(
            sub_agent.maybe_opamp_client.as_ref().unwrap(),
            TestAgent::valid_remote_config(),
            old_supervisor,
        );

        // The hash should not be persisted since it was detected as failed
        let remote_config = config_repository
            .get_remote_config(&TestAgent::id())
            .unwrap();
        assert!(remote_config.is_none());

        // Yaml config doesn't change
        config_repository.assert_no_config_for_agent(&TestAgent::id());

        assert!(new_supervisor.is_some());
    }

    #[test]
    fn test_remote_config_failed_to_failed() {
        let (config_repository, mut opamp_client) = test_mocks();

        let supervisor_builder = expect_supervisor_do_not_build();
        opamp_client.should_set_remote_config_status(RemoteConfigStatus {
            status: RemoteConfigStatuses::Failed as i32,
            last_remote_config_hash: TestAgent::failed_remote_config()
                .hash
                .to_string()
                .into_bytes(),
            error_message: TestAgent::failed_remote_config()
                .state
                .error_message()
                .cloned()
                .unwrap(),
        });

        let sub_agent = sub_agent(
            Some(opamp_client),
            supervisor_builder,
            config_repository.clone(),
        );

        let old_supervisor = Some(expect_supervisor_does_not_stop());

        let new_supervisor = sub_agent.handle_remote_config(
            sub_agent.maybe_opamp_client.as_ref().unwrap(),
            TestAgent::failed_remote_config(),
            old_supervisor,
        );

        // The hash should not be persisted since it was detected as failed
        let remote_config = config_repository
            .get_remote_config(&TestAgent::id())
            .unwrap();
        assert!(remote_config.is_none());

        // Yaml config doesn't change
        config_repository.assert_no_config_for_agent(&TestAgent::id());

        assert!(new_supervisor.is_some());
    }
    #[test]
    fn test_remote_config_reset_to_local() {
        let (config_repository, mut opamp_client) = test_mocks();

        config_repository
            .store_local(&TestAgent::id(), &TestAgent::valid_config_yaml())
            .unwrap();
        let old_remote_config = RemoteConfig {
            config: "var: some old remote".try_into().unwrap(),
            hash: Hash::from("a-hash"),
            state: ConfigState::Applied,
        };
        config_repository
            .store_remote(&TestAgent::id(), &old_remote_config)
            .unwrap();

        let supervisor_builder = expect_build_supervisor_with(TestAgent::valid_config_value());
        opamp_client.should_update_effective_config(1);
        opamp_client.should_set_remote_config_status_seq(vec![
            TestAgent::status_applying(),
            TestAgent::status_applied(),
        ]);

        let sub_agent = sub_agent(
            Some(opamp_client),
            supervisor_builder,
            config_repository.clone(),
        );

        let old_supervisor = Some(expect_supervisor_shut_down());

        let new_supervisor = sub_agent.handle_remote_config(
            sub_agent.maybe_opamp_client.as_ref().unwrap(),
            TestAgent::reset_remote_config(),
            old_supervisor,
        );

        // Now config is deleted so no hash exists.
        let remote_config = config_repository
            .get_remote_config(&TestAgent::id())
            .unwrap();
        assert!(remote_config.is_none());

        assert!(
            config_repository
                .load_remote(&TestAgent::id(), &Capabilities::default())
                .unwrap()
                .is_none()
        );

        assert!(new_supervisor.is_some());
    }
    #[test]
    fn test_remote_config_reset_to_empty_local() {
        let (config_repository, mut opamp_client) = test_mocks();

        let remote_config = RemoteConfig {
            config: TestAgent::valid_config_yaml(),
            hash: Hash::from("a-hash"),
            state: ConfigState::Applying,
        };
        config_repository
            .store_remote(&TestAgent::id(), &remote_config)
            .unwrap();

        let supervisor_builder = expect_supervisor_do_not_build();
        opamp_client.should_update_effective_config(1);
        opamp_client.should_set_remote_config_status_seq(vec![
            TestAgent::status_applying(),
            TestAgent::status_applied(),
        ]);

        let sub_agent = sub_agent(
            Some(opamp_client),
            supervisor_builder,
            config_repository.clone(),
        );

        let old_supervisor = Some(expect_supervisor_shut_down());

        let new_supervisor = sub_agent.handle_remote_config(
            sub_agent.maybe_opamp_client.as_ref().unwrap(),
            TestAgent::reset_remote_config(),
            old_supervisor,
        );

        let remote_config = config_repository
            .get_remote_config(&TestAgent::id())
            .unwrap();
        assert!(remote_config.is_none());

        assert!(
            config_repository
                .load_remote(&TestAgent::id(), &Capabilities::default())
                .unwrap()
                .is_none()
        );

        assert!(new_supervisor.is_none());
    }
    #[test]
    fn test_remote_config_reset_to_broken_local() {
        let (config_repository, mut opamp_client) = test_mocks();

        config_repository
            .store_local(&TestAgent::id(), &TestAgent::valid_config_yaml())
            .unwrap();
        let old_remote_config = RemoteConfig {
            config: "var: some old remote".try_into().unwrap(),
            hash: Hash::from("a-hash"),
            state: ConfigState::Applied,
        };
        config_repository
            .store_remote(&TestAgent::id(), &old_remote_config)
            .unwrap();

        let supervisor_builder = expect_fail_to_build_supervisor();
        opamp_client.should_update_effective_config(1);
        opamp_client.should_set_remote_config_status_seq(vec![
            TestAgent::status_applying(),
            TestAgent::status_applied(),
        ]);

        let sub_agent = sub_agent(
            Some(opamp_client),
            supervisor_builder,
            config_repository.clone(),
        );

        let old_supervisor = Some(expect_supervisor_shut_down());

        let new_supervisor = sub_agent.handle_remote_config(
            sub_agent.maybe_opamp_client.as_ref().unwrap(),
            TestAgent::reset_remote_config(),
            old_supervisor,
        );

        // Now config is deleted so no hash exists.
        let remote_config = config_repository
            .get_remote_config(&TestAgent::id())
            .unwrap();
        assert!(remote_config.is_none());

        assert!(
            config_repository
                .load_remote(&TestAgent::id(), &Capabilities::default())
                .unwrap()
                .is_none()
        );

        assert!(new_supervisor.is_none());
    }

    #[test]
    fn test_remote_config_hash_already_stored_applying_to_applied() {
        // Given a remote_config with the same hash as the stored one that is applying, it should
        // do all normal steps.
        let (config_repository, mut opamp_client) = test_mocks();
        opamp_client.should_update_effective_config(1);

        let old_remote_config = RemoteConfig {
            config: "var: some old remote".try_into().unwrap(),
            hash: TestAgent::hash(),
            state: ConfigState::Applying,
        };
        config_repository
            .store_remote(&TestAgent::id(), &old_remote_config)
            .unwrap();

        let supervisor_builder = expect_build_supervisor_with(TestAgent::valid_config_value());
        opamp_client.should_set_remote_config_status_seq(vec![
            TestAgent::status_applying(),
            TestAgent::status_applied(),
        ]);

        let sub_agent = sub_agent(
            Some(opamp_client),
            supervisor_builder,
            config_repository.clone(),
        );

        let old_supervisor = Some(expect_supervisor_shut_down());

        let new_supervisor = sub_agent.handle_remote_config(
            sub_agent.maybe_opamp_client.as_ref().unwrap(),
            TestAgent::valid_remote_config(),
            old_supervisor,
        );

        assert_remote_config(
            config_repository.deref(),
            &TestAgent::id(),
            |remote_config| {
                assert_eq!(
                    remote_config.hash.to_string(),
                    TestAgent::hash().to_string()
                );
                assert!(remote_config.state.is_applied());
            },
        );

        assert!(new_supervisor.is_some());
    }

    #[test]
    fn test_remote_config_hash_already_stored_only_report_applied() {
        // Given a remote_config with the same hash as the stored one that is applied, it should
        // keep old_supervisor and repply it again as applied.
        let (config_repository, mut opamp_client) = test_mocks();

        let old_remote_config = RemoteConfig {
            config: "var: some old remote".try_into().unwrap(),
            hash: TestAgent::hash(),
            state: ConfigState::Applied,
        };

        config_repository
            .store_remote(&TestAgent::id(), &old_remote_config)
            .unwrap();

        let supervisor_builder = expect_supervisor_do_not_build();
        opamp_client.should_set_remote_config_status_seq(vec![TestAgent::status_applied()]);

        let sub_agent = sub_agent(
            Some(opamp_client),
            supervisor_builder,
            config_repository.clone(),
        );

        let old_supervisor = Some(expect_supervisor_does_not_stop());

        let new_supervisor = sub_agent.handle_remote_config(
            sub_agent.maybe_opamp_client.as_ref().unwrap(),
            TestAgent::valid_remote_config(),
            old_supervisor,
        );

        assert_remote_config(
            config_repository.deref(),
            &TestAgent::id(),
            |remote_config| {
                assert_eq!(
                    remote_config.hash.to_string(),
                    TestAgent::hash().to_string()
                );
                assert!(remote_config.state.is_applied());
            },
        );

        assert!(new_supervisor.is_some());
    }

    #[test]
    fn test_bootstrap_empty_config() {
        let (config_repository, mut opamp_client) = test_mocks();
        opamp_client.should_update_effective_config(1);

        let supervisor_builder = expect_supervisor_do_not_build();

        let supervisor =
            sub_agent(Some(opamp_client), supervisor_builder, config_repository).init_supervisor();

        assert!(supervisor.is_none());
    }
    #[test]
    fn test_bootstrap_local_config() {
        let (config_repository, mut opamp_client) = test_mocks();

        config_repository
            .store_local(&TestAgent::id(), &TestAgent::valid_config_yaml())
            .unwrap();

        let supervisor_builder = expect_build_supervisor_with(TestAgent::valid_config_value());

        opamp_client.should_update_effective_config(1);

        let supervisor =
            sub_agent(Some(opamp_client), supervisor_builder, config_repository).init_supervisor();

        assert!(supervisor.is_some())
    }
    #[test]
    fn test_bootstrap_remote_config_applied_to_applied() {
        let (config_repository, mut opamp_client) = test_mocks();

        let remote_config = RemoteConfig {
            config: TestAgent::valid_config_yaml(),
            hash: TestAgent::hash(),
            state: ConfigState::Applied,
        };
        config_repository
            .store_remote(&TestAgent::id(), &remote_config)
            .unwrap();

        let supervisor_builder = expect_build_supervisor_with(TestAgent::valid_config_value());

        opamp_client.should_update_effective_config(1);

        let supervisor = sub_agent(
            Some(opamp_client),
            supervisor_builder,
            config_repository.clone(),
        )
        .init_supervisor();

        assert!(supervisor.is_some());

        assert_remote_config(
            config_repository.deref(),
            &TestAgent::id(),
            |remote_config| assert!(remote_config.state.is_applied()),
        );
    }

    #[test]
    fn test_bootstrap_remote_config_applying_to_applied() {
        let (config_repository, mut opamp_client) = test_mocks();

        let remote_config = RemoteConfig {
            config: TestAgent::valid_config_yaml(),
            hash: TestAgent::hash(),
            state: ConfigState::Applying,
        };
        config_repository
            .store_remote(&TestAgent::id(), &remote_config)
            .unwrap();

        let supervisor_builder = expect_build_supervisor_with(TestAgent::valid_config_value());

        opamp_client.should_update_effective_config(1);
        opamp_client.should_set_remote_config_status(TestAgent::status_applied());

        let supervisor = sub_agent(
            Some(opamp_client),
            supervisor_builder,
            config_repository.clone(),
        )
        .init_supervisor();

        assert!(supervisor.is_some());

        assert_remote_config(
            config_repository.deref(),
            &TestAgent::id(),
            |remote_config| assert!(remote_config.state.is_applied()),
        );
    }
    #[test]
    fn test_bootstrap_remote_config_applying_to_failed() {
        let (config_repository, mut opamp_client) = test_mocks();

        let remote_config = RemoteConfig {
            config: TestAgent::valid_config_yaml(),
            hash: TestAgent::hash(),
            state: ConfigState::Applying,
        };
        config_repository
            .store_remote(&TestAgent::id(), &remote_config)
            .unwrap();

        let supervisor_builder = expect_fail_to_build_supervisor();

        opamp_client.should_set_remote_config_status(TestAgent::status_failed());

        let supervisor = sub_agent(
            Some(opamp_client),
            supervisor_builder,
            config_repository.clone(),
        )
        .init_supervisor();

        assert!(supervisor.is_none());

        assert_remote_config(
            config_repository.deref(),
            &TestAgent::id(),
            |remote_config| assert!(remote_config.state.is_failed()),
        );
    }
    #[test]
    fn test_bootstrap_stored_remote_config_failed_to_failed() {
        let (config_repository, mut opamp_client) = test_mocks();

        // In case a remote_config was marked as failed after being in applying state,
        // if init_supervisor is called again, the supervisor will use the current config even if
        // it doesn't work but won't report the failure again since the hash was already reported.
        // The remote config will always be used not falling back to local,
        // if it has been stored in the repository, even if the hash is failed, but a remote_config
        // detected as failed by any validator, won't be saved into the repository at all.
        let hash = TestAgent::hash();
        let state = ConfigState::Failed {
            error_message: "some failure".to_string(),
        };
        let input_remote_config = RemoteConfig {
            config: "var: valid".try_into().unwrap(),
            hash,
            state,
        };
        config_repository
            .store_remote(&TestAgent::id(), &input_remote_config)
            .unwrap();

        let supervisor_builder = expect_build_supervisor_with(TestAgent::valid_config_value());

        opamp_client.should_update_effective_config(1);

        let supervisor = sub_agent(
            Some(opamp_client),
            supervisor_builder,
            config_repository.clone(),
        )
        .init_supervisor();

        assert!(supervisor.is_some());

        assert_remote_config(
            config_repository.deref(),
            &TestAgent::id(),
            |remote_config| assert!(remote_config.state.is_failed()),
        );
    }

    // Helpers

    fn assert_remote_config(
        config_repository: &impl ConfigRepository,
        agent_id: &AgentID,
        assertion: impl FnOnce(&RemoteConfig),
    ) {
        let remote_config = config_repository
            .get_remote_config(agent_id)
            .expect("assert_remote_config: error on `get_remote_config")
            .expect("assert_remote_config: remote config not found");

        assertion(&remote_config);
    }
}
