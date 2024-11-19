use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;
use tracing::{debug, error};

use crate::common::clientstate::{ClientSyncedState, SyncedStateError};
use crate::{
    opamp::proto::{
        AgentCapabilities::*, AgentToServer, ConnectionSettingsOffers, OtherConnectionSettings,
        ServerToAgent, ServerToAgentFlags, TelemetryConnectionSettings,
    },
    operation::{
        callbacks::{Callbacks, MessageData},
        capabilities::Capabilities,
    },
};

use super::nextmessage::NextMessage;
use crate::opamp::proto::AgentCapabilities;

#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("Error while acquiring read-write lock")]
    PoisonError,

    /// Represents a synced state error.
    #[error("synced state error: `{0}`")]
    SyncedStateError(#[from] SyncedStateError),
}

#[derive(Debug, PartialEq)]
pub(crate) enum ProcessResult {
    Synced,
    NeedsResend,
}

/// Asynchronously parses a ServerToAgent message and calls the corresponding callbacks.
/// A ServerToAgent message might ask for a new AgentToServer send, which will be reflected in
/// ProcessResult::NeedsResend.
///
/// # Arguments
///
/// * `msg` - The ServerToAgent message.
/// * `callbacks` - A reference to the `Callbacks` object containing user-provided callbacks.
/// * `synced_state` - A reference to the `ClientSyncedState` object holding the client state.
/// * `capabilities` - A reference to the `Capabilities` object that describes agent capabilities.
/// * `next_message` - An Arc<RwLock<NextMessage>> containing the next message to send.
///
/// # Returns
///
/// A `Result` containing a `ProcessResult` or a `ProcessError`.
pub(crate) fn process_message<C: Callbacks>(
    msg: ServerToAgent,
    callbacks: &C,
    synced_state: &ClientSyncedState,
    capabilities: &Capabilities,
    next_message: Arc<RwLock<NextMessage>>,
) -> Result<ProcessResult, ProcessError> {
    if msg
        .command
        .as_ref()
        .filter(|_| report_capability("Command", capabilities, AcceptsRestartCommand))
        .map(|c| {
            callbacks
                .on_command(c)
                .map_err(|e| error!("on_command callback returned an error: {e}"))
        })
        .is_some()
    {
        return Ok(ProcessResult::Synced);
    }

    let msg_data = message_data(&msg, capabilities);

    if let Some(id) = &msg_data.agent_identification {
        next_message
            .write()
            .map_err(|_| ProcessError::PoisonError)?
            .update(move |msg: &mut AgentToServer| {
                msg.instance_uid.clone_from(&id.new_instance_uid);
            });
    }

    callbacks.on_message(msg_data);

    // FIXME: revisit this once we clarify the opamp settings update flows
    // if let Some(o) = msg
    //     .connection_settings
    //     .and_then(|s| s.opamp)
    //     .filter(|_| report_capability("opamp", capabilities, AcceptsOpAmpConnectionSettings))
    // {
    //     callbacks
    //         .on_opamp_connection_settings(&o)
    //         .is_ok()
    //         .then(|| callbacks.on_opamp_connection_settings_accepted(&o));
    // }

    if let Some(e) = msg.error_response {
        error!("Received an error from server: {e:?}");
    }

    rcv_flags(synced_state, msg.flags, next_message, callbacks)
}

// An async function handling received flags.
fn rcv_flags<C: Callbacks>(
    state: &ClientSyncedState,
    flags: u64,
    next_message: Arc<RwLock<NextMessage>>,
    callbacks: &C,
) -> Result<ProcessResult, ProcessError> {
    let can_report_full_state = flags & ServerToAgentFlags::ReportFullState as u64 != 0;
    if can_report_full_state {
        let agent_description = state.agent_description()?;
        let health = state.health()?;
        let remote_config_status = state.remote_config_status()?;
        let package_statuses = state.package_statuses()?;

        next_message
            .write()
            .map_err(|_| ProcessError::PoisonError)?
            .update(|msg: &mut AgentToServer| {
                msg.agent_description = agent_description;
                msg.health = health;
                msg.remote_config_status = remote_config_status;
                msg.package_statuses = package_statuses;
                msg.effective_config = callbacks
                    .get_effective_config()
                    .map_err(|e| error!("Cannot get effective config: {e}"))
                    .ok();
            });
        Ok(ProcessResult::NeedsResend)
    } else {
        Ok(ProcessResult::Synced)
    }
}

// A helper function that returns a MessageData object containing relevant fields based on agent capabilities.
fn message_data(msg: &ServerToAgent, capabilities: &Capabilities) -> MessageData {
    let remote_config = msg
        .remote_config
        .clone()
        .filter(|_| report_capability("remote_config", capabilities, AcceptsRemoteConfig));

    let (own_metrics, own_traces, own_logs, other_connection_settings) =
        get_telemetry_connection_settings(msg.connection_settings.clone(), capabilities);

    // TODO: package_syncer feature
    // let packages_available;
    // let package_syncer;
    // if let Some(packages_available) = msg.packages_available.filter(|_| report_capability("PackagesAvailable", capabilities, AgentCapabilities::AcceptsPackages)) {
    //     let packages_available = packages_available;
    //     let package_syncer = PackageSyncer::new(packages_available, self.sender.clone());
    // }

    let agent_identification = msg.agent_identification.clone().filter(|id| {
        let is_empty_string = id.new_instance_uid.is_empty();
        if is_empty_string {
            debug!("Empty instance UID is not allowed. Ignoring agent identification.");
        }
        !is_empty_string
    });

    MessageData {
        remote_config,
        own_metrics,
        own_traces,
        own_logs,
        other_connection_settings,
        agent_identification,
        // packages_available,
        // package_syncer,
    }
}

// A helper function checking if an agent has a specified capability and reports information accordingly.
fn report_capability(
    opt_name: &str,
    capabilities: &Capabilities,
    capability: AgentCapabilities,
) -> bool {
    let has_cap = capabilities.has_capability(capability);
    if !has_cap {
        debug!(
            "Ignoring {opt_name}, agent does not have {} capability",
            capability.as_str_name()
        );
    }
    has_cap
}

// Type alias for connection settings.
type ConnectionSettings = (
    Option<TelemetryConnectionSettings>,
    Option<TelemetryConnectionSettings>,
    Option<TelemetryConnectionSettings>,
    Option<HashMap<String, OtherConnectionSettings>>,
);

// A helper function that extracts the telemetry connection settings based on agent capabilities.
fn get_telemetry_connection_settings(
    settings: Option<ConnectionSettingsOffers>,
    capabilities: &Capabilities,
) -> ConnectionSettings {
    if let Some(s) = settings {
        (
            s.own_metrics
                .filter(|_| report_capability("own_metrics", capabilities, ReportsOwnMetrics)),
            s.own_traces
                .filter(|_| report_capability("own_traces", capabilities, ReportsOwnTraces)),
            s.own_logs
                .filter(|_| report_capability("own_logs", capabilities, ReportsOwnLogs)),
            Some(s.other_connections).filter(|_| {
                report_capability(
                    "other_connections",
                    capabilities,
                    AcceptsOtherConnectionSettings,
                )
            }),
        )
    } else {
        (None, None, None, None)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::capabilities;
    use crate::common::clientstate::ClientSyncedState;
    use crate::opamp::proto::{
        any_value::Value, AgentConfigMap, AgentDescription, AgentIdentification, AgentRemoteConfig,
        AnyValue, ComponentHealth, KeyValue, PackageStatuses, RemoteConfigStatus,
        ServerErrorResponse, ServerToAgent, ServerToAgentCommand,
    };
    use crate::operation::callbacks::test::MockCallbacksMockall;
    use tracing_test::traced_test;

    #[test]
    fn receive_command() {
        // The idea here is that we construct all arguments passed to `receive`, and then check for:
        // 1. The appropriate callbacks have been called
        // 2. The return value is what we expect

        let server_to_agent = ServerToAgent {
            command: Some(ServerToAgentCommand::default()), // An arbitrary command
            ..ServerToAgent::default()
        };
        let mut callbacks = MockCallbacksMockall::new();
        let synced_state = ClientSyncedState::default();
        let capabilities = capabilities!(AgentCapabilities::AcceptsRestartCommand);
        let next_message = Arc::new(RwLock::new(NextMessage::default()));

        callbacks.should_on_command(&ServerToAgentCommand::default()); // I expect on_command to be called

        let res = process_message(
            server_to_agent,
            &callbacks,
            &synced_state,
            &capabilities,
            next_message,
        );

        assert!(res.is_ok());
        assert_eq!(res.unwrap(), ProcessResult::Synced);
    }

    #[test]
    fn receive_command_but_not_capable() {
        let server_to_agent = ServerToAgent {
            command: Some(ServerToAgentCommand::default()), // An arbitrary command
            ..ServerToAgent::default()
        };
        let mut callbacks = MockCallbacksMockall::new();
        let synced_state = ClientSyncedState::default();
        let capabilities = capabilities!(); // We don't have the capability to be restarted
        let next_message = Arc::new(RwLock::new(NextMessage::default()));

        callbacks.should_not_on_command(); // I expect on_command to NOT be called
        callbacks.should_on_message(MessageData::default());

        let res = process_message(
            server_to_agent,
            &callbacks,
            &synced_state,
            &capabilities,
            next_message,
        );

        assert!(res.is_ok());
        assert_eq!(res.unwrap(), ProcessResult::Synced);
    }

    #[test]
    fn receive_agent_identification() {
        let actual_message: Vec<u8> = "some_agent_uid".into();

        let server_to_agent = ServerToAgent {
            agent_identification: Some(AgentIdentification {
                new_instance_uid: actual_message.to_owned(),
            }),
            ..ServerToAgent::default()
        };
        let mut callbacks = MockCallbacksMockall::new();
        let synced_state = ClientSyncedState::default();
        let capabilities = capabilities!();
        let next_message = Arc::new(RwLock::new(NextMessage::default()));

        callbacks.should_not_on_command(); // I expect on_command to NOT be called

        let msg_data = message_data(&server_to_agent, &capabilities);
        callbacks.should_on_message(msg_data);

        let res = process_message(
            server_to_agent,
            &callbacks,
            &synced_state,
            &capabilities,
            next_message.clone(),
        );

        let expected_message = next_message.write().unwrap().pop();
        assert_eq!(expected_message.instance_uid, actual_message);

        assert!(res.is_ok());
        assert_eq!(res.unwrap(), ProcessResult::Synced);
    }

    #[test]
    /// Expected to not call message update if instance_uid is not present in ServerToAgent message
    ///
    fn receive_no_agent_identification() {
        let agent_uid: Vec<u8> = "some_uid".into();

        let server_to_agent = ServerToAgent::default();
        let mut callbacks = MockCallbacksMockall::new();
        let synced_state = ClientSyncedState::default();
        let capabilities = capabilities!();
        let next_message = Arc::new(RwLock::new(NextMessage::new(AgentToServer {
            instance_uid: agent_uid.to_owned(),
            ..AgentToServer::default()
        })));

        callbacks.should_not_on_command(); // I expect on_command to NOT be called

        let msg_data = message_data(&server_to_agent, &capabilities);
        callbacks.should_on_message(msg_data);

        let res = process_message(
            server_to_agent,
            &callbacks,
            &synced_state,
            &capabilities,
            next_message.clone(),
        );

        let expected_message = next_message.write().unwrap().pop();
        assert_eq!(expected_message.instance_uid, agent_uid);

        assert!(res.is_ok());
        assert_eq!(res.unwrap(), ProcessResult::Synced);
    }

    #[test]
    #[traced_test]
    fn receive_emits_error() {
        const ERROR_RESPONSE: &str = "RANDOM ERROR";
        let err_response = ServerErrorResponse {
            error_message: ERROR_RESPONSE.to_string(),
            ..ServerErrorResponse::default()
        };
        let server_to_agent = ServerToAgent {
            error_response: Some(err_response.clone()),
            ..ServerToAgent::default()
        };
        let mut callbacks = MockCallbacksMockall::new();
        let synced_state = ClientSyncedState::default();
        let capabilities = capabilities!();
        let next_message = Arc::new(RwLock::new(NextMessage::default()));

        callbacks.should_not_on_command(); // I expect on_command to NOT be called

        let msg_data = message_data(&server_to_agent, &capabilities);
        callbacks.should_on_message(msg_data);

        let _res = process_message(
            server_to_agent,
            &callbacks,
            &synced_state,
            &capabilities,
            next_message.clone(),
        );

        assert!(logs_contain(&format!(
            "Received an error from server: {:?}",
            err_response
        )));
    }

    #[test]
    fn test_message_data() {
        let msg = ServerToAgent::default();

        let capabilities = Capabilities::default();

        let message_data = message_data(&msg, &capabilities);

        assert_eq!(message_data.remote_config, None);
        assert_eq!(message_data.own_metrics, None);
        assert_eq!(message_data.own_traces, None);
        assert_eq!(message_data.own_logs, None);
        assert_eq!(message_data.other_connection_settings, None);
        assert_eq!(message_data.agent_identification, None);
    }

    #[test]
    fn test_message_data_with_remote_config() {
        let remote_config = AgentRemoteConfig {
            config: Some(AgentConfigMap {
                config_map: Default::default(),
            }),
            config_hash: Default::default(),
        };

        let msg = ServerToAgent {
            remote_config: Some(remote_config.clone()),
            ..Default::default()
        };

        let capabilities = capabilities!(AgentCapabilities::AcceptsRemoteConfig);

        let message_data = message_data(&msg, &capabilities);

        assert_eq!(message_data.remote_config, Some(remote_config));
        assert_eq!(message_data.own_metrics, None);
        assert_eq!(message_data.own_traces, None);
        assert_eq!(message_data.own_logs, None);
        assert_eq!(message_data.other_connection_settings, None);
        assert_eq!(message_data.agent_identification, None);
    }

    #[test]
    fn test_message_data_with_agent_identification() {
        let agent_identification = AgentIdentification {
            new_instance_uid: "test-instance-uid".into(),
        };

        let msg = ServerToAgent {
            agent_identification: Some(agent_identification),
            ..Default::default()
        };

        let capabilities = capabilities!();

        let message_data = message_data(&msg, &capabilities);

        assert_eq!(message_data.remote_config, None);
        assert_eq!(message_data.own_metrics, None);
        assert_eq!(message_data.own_traces, None);
        assert_eq!(message_data.own_logs, None);
        assert_eq!(message_data.other_connection_settings, None);
        assert_eq!(
            message_data.agent_identification,
            Some(AgentIdentification {
                new_instance_uid: "test-instance-uid".into()
            })
        );
    }

    #[test]
    fn test_message_data_with_agent_identification_and_empty_instance_uid() {
        let agent_identification = AgentIdentification {
            new_instance_uid: "".into(),
        };

        let msg = ServerToAgent {
            agent_identification: Some(agent_identification),
            ..Default::default()
        };

        let capabilities = capabilities!();

        let message_data = message_data(&msg, &capabilities);

        assert_eq!(message_data.remote_config, None);
        assert_eq!(message_data.own_metrics, None);
        assert_eq!(message_data.own_traces, None);
        assert_eq!(message_data.own_logs, None);
        assert_eq!(message_data.other_connection_settings, None);
        assert_eq!(message_data.agent_identification, None);
    }

    #[test]
    fn test_get_telemetry_connection_settings_metrics() {
        // Test with reporting metrics
        let capabilities = capabilities!(ReportsOwnMetrics);
        let settings = Some(ConnectionSettingsOffers {
            own_metrics: Some(TelemetryConnectionSettings::default()),
            own_traces: Some(TelemetryConnectionSettings::default()),
            own_logs: Some(TelemetryConnectionSettings::default()),
            other_connections: HashMap::default(),
            ..Default::default()
        });
        let expected = (
            Some(TelemetryConnectionSettings::default()),
            None,
            None,
            None,
        );
        assert_eq!(
            get_telemetry_connection_settings(settings, &capabilities),
            expected
        );
    }

    #[test]
    fn test_get_telemetry_connection_settings_traces() {
        // Test with reporting traces
        let capabilities = capabilities!(ReportsOwnTraces);
        let settings = Some(ConnectionSettingsOffers {
            own_metrics: Some(TelemetryConnectionSettings::default()),
            own_traces: Some(TelemetryConnectionSettings::default()),
            own_logs: Some(TelemetryConnectionSettings::default()),
            other_connections: HashMap::default(),
            ..Default::default()
        });
        let expected = (
            None,
            Some(TelemetryConnectionSettings::default()),
            None,
            None,
        );
        assert_eq!(
            get_telemetry_connection_settings(settings, &capabilities),
            expected
        );
    }

    #[test]
    fn test_get_telemetry_connection_settings_logs() {
        // Test with reporting logs
        let capabilities = capabilities!(ReportsOwnLogs);
        let settings = Some(ConnectionSettingsOffers {
            own_metrics: Some(TelemetryConnectionSettings::default()),
            own_traces: Some(TelemetryConnectionSettings::default()),
            own_logs: Some(TelemetryConnectionSettings::default()),
            other_connections: HashMap::default(),
            ..Default::default()
        });
        let expected = (
            None,
            None,
            Some(TelemetryConnectionSettings::default()),
            None,
        );
        assert_eq!(
            get_telemetry_connection_settings(settings, &capabilities),
            expected
        );
    }

    #[test]
    fn test_get_telemetry_connection_settings_other_conns() {
        // Test with reporting other connections
        let other_conn: HashMap<String, OtherConnectionSettings> =
            [("example".to_string(), OtherConnectionSettings::default())]
                .iter()
                .cloned()
                .collect();
        let capabilities = capabilities!(AcceptsOtherConnectionSettings);
        let settings = Some(ConnectionSettingsOffers {
            own_metrics: Some(TelemetryConnectionSettings::default()),
            own_traces: Some(TelemetryConnectionSettings::default()),
            own_logs: Some(TelemetryConnectionSettings::default()),
            other_connections: other_conn.clone(),
            ..Default::default()
        });
        let expected = (None, None, None, Some(other_conn));
        assert_eq!(
            get_telemetry_connection_settings(settings, &capabilities),
            expected
        );
    }

    #[test]
    fn test_get_telemetry_connection_settings_no_capabilities() {
        // Test with no capabilities
        let capabilities = capabilities!();
        let settings = Some(ConnectionSettingsOffers {
            own_metrics: Some(TelemetryConnectionSettings::default()),
            own_traces: Some(TelemetryConnectionSettings::default()),
            own_logs: Some(TelemetryConnectionSettings::default()),
            other_connections: HashMap::default(),
            ..Default::default()
        });
        let expected = (None, None, None, None);
        assert_eq!(
            get_telemetry_connection_settings(settings, &capabilities),
            expected
        );

        // Test with no settings
        let capabilities = capabilities!();
        let settings = None;
        let expected = (None, None, None, None);
        assert_eq!(
            get_telemetry_connection_settings(settings, &capabilities),
            expected
        );
    }

    #[test]
    fn test_get_telemetry_connection_settings_several_capabilities() {
        let capabilities = capabilities!(ReportsOwnMetrics, ReportsOwnTraces, ReportsOwnLogs);
        let settings = Some(ConnectionSettingsOffers {
            own_metrics: Some(TelemetryConnectionSettings::default()),
            own_traces: Some(TelemetryConnectionSettings::default()),
            own_logs: Some(TelemetryConnectionSettings::default()),
            other_connections: HashMap::default(),
            ..Default::default()
        });
        let expected = (
            Some(TelemetryConnectionSettings::default()),
            Some(TelemetryConnectionSettings::default()),
            Some(TelemetryConnectionSettings::default()),
            None,
        );
        assert_eq!(
            get_telemetry_connection_settings(settings, &capabilities),
            expected
        );
    }

    #[test]
    fn test_rcv_flags_needs_resend() {
        let mut callbacks_mock = MockCallbacksMockall::new();
        let state = ClientSyncedState::default();

        let expected_agent_description = AgentDescription {
            identifying_attributes: vec![KeyValue {
                key: "thing".to_string(),
                value: Some(AnyValue {
                    value: Some(Value::StringValue("thing_value".to_string())),
                }),
            }],
            non_identifying_attributes: vec![],
        };

        state
            .set_agent_description(expected_agent_description.clone())
            .unwrap();

        let expected_health = ComponentHealth {
            healthy: true,
            last_error: "".to_string(),
            ..Default::default()
        };

        state.set_health(expected_health.clone()).unwrap();

        let expected_remote_config_status = RemoteConfigStatus {
            last_remote_config_hash: vec![],
            status: 2,
            error_message: "".to_string(),
        };

        state
            .set_remote_config_status(expected_remote_config_status.clone())
            .unwrap();

        let expected_package_statuses = PackageStatuses {
            packages: HashMap::from([]),
            server_provided_all_packages_hash: vec![],
            error_message: "some error".to_string(),
        };

        state
            .set_package_statuses(expected_package_statuses.clone())
            .unwrap();

        let flags = ServerToAgentFlags::ReportFullState as u64;
        let next_message: Arc<RwLock<NextMessage>> = Arc::new(RwLock::new(NextMessage::default()));

        callbacks_mock.should_get_effective_config();

        let result: Result<ProcessResult, ProcessError> =
            rcv_flags(&state, flags, next_message.clone(), &callbacks_mock);

        let mut lock = next_message.write().unwrap();
        let message = (*lock).pop();

        assert_eq!(
            expected_agent_description,
            message.agent_description.unwrap()
        );

        assert!(message.health.unwrap().is_same_as(&expected_health));

        assert_eq!(
            expected_remote_config_status,
            message.remote_config_status.unwrap()
        );

        assert_eq!(expected_package_statuses, message.package_statuses.unwrap());
        assert!(matches!(result, Ok(ProcessResult::NeedsResend)));
    }

    #[test]
    fn test_rcv_flags_synced() {
        let mut callbacks_mock = MockCallbacksMockall::new();
        let state = ClientSyncedState::default();
        let next_message: Arc<RwLock<NextMessage>> = Arc::new(RwLock::new(NextMessage::default()));

        callbacks_mock.should_not_get_effective_config();

        let unset_flag = 0;
        let result_synced: Result<ProcessResult, ProcessError> =
            rcv_flags(&state, unset_flag, next_message.clone(), &callbacks_mock);

        assert!(matches!(result_synced, Ok(ProcessResult::Synced)));
    }
}
