use crate::agent_control::config::K8sConfig;
use crate::agent_control::config_storer::loader_storer::AgentControlConfigLoader;
use crate::agent_control::config_storer::store::AgentControlConfigStore;
use crate::agent_control::config_validator::RegistryDynamicConfigValidator;
use crate::agent_control::defaults::{
    AGENT_CONTROL_VERSION, FLEET_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY,
    OPAMP_AGENT_VERSION_ATTRIBUTE_KEY, OPAMP_CHART_VERSION_ATTRIBUTE_KEY,
};
use crate::agent_control::run::AgentControlRunner;
use crate::agent_control::AgentControl;
use crate::agent_type::environment::Environment;
use crate::agent_type::render::renderer::TemplateRenderer;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::opamp::effective_config::loader::DefaultEffectiveConfigLoaderBuilder;
use crate::opamp::instance_id::getter::InstanceIDWithIdentifiersGetter;
use crate::opamp::instance_id::Identifiers;
use crate::opamp::operations::build_opamp_with_channel;
use crate::opamp::remote_config::validators::regexes::RegexValidator;
use crate::opamp::remote_config::validators::values::ValuesValidator;
use crate::opamp::remote_config::validators::SupportedRemoteConfigValidator;
use crate::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use crate::sub_agent::event_handler::opamp::remote_config_handler::AgentRemoteConfigHandler;
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::k8s::builder::SupervisorBuilderK8s;
use crate::sub_agent::supervisor::assembler::AgentSupervisorAssembler;
use crate::{
    agent_control::error::AgentError,
    opamp::{
        client_builder::DefaultOpAMPClientBuilder, hash_repository::k8s::HashRepositoryConfigMap,
        instance_id,
    },
};
use crate::{
    k8s::{garbage_collector::NotStartedK8sGarbageCollector, store::K8sStore},
    sub_agent::k8s::builder::K8sSubAgentBuilder,
    values::k8s::YAMLConfigRepositoryConfigMap,
};
use opamp_client::operation::settings::DescriptionValueType;
use resource_detection::system::hostname::HostnameGetter;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

impl AgentControlRunner {
    pub fn run(self) -> Result<(), AgentError> {
        info!("Starting the k8s client");
        let k8s_client = Arc::new(
            SyncK8sClient::try_new(self.runtime, self.k8s_config.namespace.clone())
                .map_err(|e| AgentError::ExternalError(e.to_string()))?,
        );
        let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));

        debug!("Initialising yaml_config_repository");
        let yaml_config_repository = if self.opamp_http_builder.is_some() {
            Arc::new(YAMLConfigRepositoryConfigMap::new(k8s_store.clone()).with_remote())
        } else {
            Arc::new(YAMLConfigRepositoryConfigMap::new(k8s_store.clone()))
        };

        let config_storer = Arc::new(AgentControlConfigStore::new(yaml_config_repository.clone()));

        let agent_control_config = config_storer.load()?;

        let fleet_id = agent_control_config
            .fleet_control
            .as_ref()
            .map(|c| c.fleet_id.clone())
            .unwrap_or_default();

        let identifiers =
            instance_id::get_identifiers(self.k8s_config.cluster_name.clone(), fleet_id);
        info!("Instance Identifiers: {}", identifiers);

        let mut non_identifying_attributes =
            agent_control_opamp_non_identifying_attributes(&identifiers);
        non_identifying_attributes.insert(
            "cluster.name".to_string(),
            self.k8s_config.cluster_name.clone().into(),
        );
        let additional_identifying_attributes =
            agent_control_additional_opamp_identifying_attributes(&self.k8s_config);

        let instance_id_getter = InstanceIDWithIdentifiersGetter::new_k8s_instance_id_getter(
            k8s_store.clone(),
            identifiers,
        );

        let opamp_client_builder = self.opamp_http_builder.map(|http_builder| {
            DefaultOpAMPClientBuilder::new(
                http_builder,
                DefaultEffectiveConfigLoaderBuilder::new(yaml_config_repository.clone()),
                self.opamp_poll_interval,
            )
        });

        // Build and start AC OpAMP client
        let (maybe_client, maybe_opamp_consumer) = opamp_client_builder
            .as_ref()
            .map(|builder| {
                build_opamp_with_channel(
                    builder,
                    &instance_id_getter,
                    &AgentIdentity::new_agent_control_identity(),
                    additional_identifying_attributes,
                    non_identifying_attributes,
                )
            })
            // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
            .transpose()?
            .map(|(client, consumer)| (Some(client), Some(consumer)))
            .unwrap_or_default();

        // Disable startup check for sub-agents OpAMP client builder
        let opamp_client_builder = opamp_client_builder.map(|b| b.with_startup_check_disabled());

        let template_renderer = TemplateRenderer::default();

        let agents_assembler = Arc::new(LocalEffectiveAgentsAssembler::new(
            yaml_config_repository.clone(),
            self.agent_type_registry.clone(),
            template_renderer,
        ));

        let hash_repository = Arc::new(HashRepositoryConfigMap::new(k8s_store.clone()));

        let supervisor_builder =
            SupervisorBuilderK8s::new(k8s_client.clone(), self.k8s_config.clone());

        let supervisor_assembler = AgentSupervisorAssembler::new(
            hash_repository.clone(),
            supervisor_builder,
            agents_assembler.clone(),
            Environment::K8s,
        );

        let remote_config_validators = vec![
            SupportedRemoteConfigValidator::Signature(self.signature_validator),
            SupportedRemoteConfigValidator::Regex(RegexValidator::default()),
            SupportedRemoteConfigValidator::Values(ValuesValidator::new(
                agents_assembler,
                Environment::K8s,
            )),
        ];

        let remote_config_handler = AgentRemoteConfigHandler::new(
            remote_config_validators,
            hash_repository.clone(),
            yaml_config_repository.clone(),
        );

        info!("Creating the k8s sub_agent builder");
        let sub_agent_builder = K8sSubAgentBuilder::new(
            opamp_client_builder.as_ref(),
            &instance_id_getter,
            self.k8s_config.clone(),
            Arc::new(supervisor_assembler),
            Arc::new(remote_config_handler),
        );

        let gcc = NotStartedK8sGarbageCollector::new(
            config_storer.clone(),
            k8s_client,
            self.k8s_config.cr_type_meta,
            self.garbage_collector_interval,
        );
        let _started_gcc = gcc.start();

        let dynamic_config_validator =
            RegistryDynamicConfigValidator::new(self.agent_type_registry);

        AgentControl::new(
            maybe_client,
            hash_repository,
            sub_agent_builder,
            config_storer,
            self.agent_control_publisher,
            self.sub_agent_publisher,
            self.application_event_consumer,
            maybe_opamp_consumer,
            dynamic_config_validator,
        )
        .run()
    }
}

pub fn agent_control_opamp_non_identifying_attributes(
    identifiers: &Identifiers,
) -> HashMap<String, DescriptionValueType> {
    let hostname = HostnameGetter::default()
        .get()
        .unwrap_or_else(|e| {
            error!("cannot retrieve hostname: {}", e.to_string());
            std::ffi::OsString::from("unknown_hostname")
        })
        .to_string_lossy()
        .to_string();

    HashMap::from([
        (
            HOST_NAME_ATTRIBUTE_KEY.to_string(),
            DescriptionValueType::String(hostname),
        ),
        (
            FLEET_ID_ATTRIBUTE_KEY.to_string(),
            DescriptionValueType::String(identifiers.fleet_id.clone()),
        ),
    ])
}

fn agent_control_additional_opamp_identifying_attributes(
    k8s_config: &K8sConfig,
) -> HashMap<String, DescriptionValueType> {
    let mut attributes = HashMap::from([(
        OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
        DescriptionValueType::String(AGENT_CONTROL_VERSION.to_string()),
    )]);

    if k8s_config.chart_version.is_empty() {
        warn!("Agent Control chart version was not set, it will not be reported");
        return attributes;
    }

    let chart_version = k8s_config.chart_version.to_string();

    attributes.insert(
        OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
        DescriptionValueType::String(chart_version),
    );

    attributes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_control_additional_opamp_identifying_attributes_chart_version_unset() {
        let k8s_config = K8sConfig::default();
        let expected = HashMap::from([(
            "agent.version".to_string(),
            DescriptionValueType::String(AGENT_CONTROL_VERSION.to_string()),
        )]);
        assert_eq!(
            agent_control_additional_opamp_identifying_attributes(&k8s_config),
            expected
        );
    }

    #[test]
    fn test_agent_control_additional_opamp_identifying_attributes_chart_version_set() {
        let k8s_config = K8sConfig {
            chart_version: "1.2.3".to_string(),
            ..Default::default()
        };
        let expected = HashMap::from([
            (
                "agent.version".to_string(),
                DescriptionValueType::String(AGENT_CONTROL_VERSION.to_string()),
            ),
            (
                "chart.version".to_string(),
                DescriptionValueType::String("1.2.3".to_string()),
            ),
        ]);
        assert_eq!(
            agent_control_additional_opamp_identifying_attributes(&k8s_config),
            expected
        );
    }
}
