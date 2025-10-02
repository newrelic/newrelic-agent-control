use crate::agent_control::AgentControl;
use crate::agent_control::config::{AgentControlConfigError, K8sConfig, helmrelease_v2_type_meta};
use crate::agent_control::config_repository::repository::AgentControlConfigLoader;
use crate::agent_control::config_repository::store::AgentControlConfigStore;
use crate::agent_control::config_validator::RegistryDynamicConfigValidator;
use crate::agent_control::config_validator::k8s::K8sReleaseNamesConfigValidator;
use crate::agent_control::defaults::{
    AGENT_CONTROL_VERSION, FLEET_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY,
    OPAMP_AC_CHART_VERSION_ATTRIBUTE_KEY, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
    OPAMP_CD_CHART_VERSION_ATTRIBUTE_KEY,
};
use crate::agent_control::health_checker::k8s::agent_control_health_checker_builder;
use crate::agent_control::http_server::runner::Runner;
use crate::agent_control::resource_cleaner::ResourceCleanerError;
use crate::agent_control::resource_cleaner::k8s_garbage_collector::K8sGarbageCollector;
use crate::agent_control::run::AgentControlRunner;
use crate::agent_control::version_updater::k8s::K8sACUpdater;
use crate::agent_type::render::persister::config_persister_file::ConfigurationPersisterFile;
use crate::agent_type::render::renderer::TemplateRenderer;
use crate::agent_type::variable::Variable;
use crate::agent_type::version_config::{
    AGENT_CONTROL_VERSION_CHECKER_INITIAL_DELAY, VersionCheckerInterval,
};
use crate::event::AgentControlInternalEvent;
use crate::event::channel::{EventPublisher, pub_sub};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::opamp::effective_config::loader::DefaultEffectiveConfigLoaderBuilder;
use crate::opamp::instance_id::getter::InstanceIDWithIdentifiersGetter;
use crate::opamp::instance_id::k8s::getter::{Identifiers, get_identifiers};
use crate::opamp::operations::build_opamp_with_channel;
use crate::opamp::remote_config::validators::SupportedRemoteConfigValidator;
use crate::opamp::remote_config::validators::regexes::RegexValidator;
use crate::secrets_provider::SecretsProviders;
use crate::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::k8s::builder::SupervisorBuilderK8s;
use crate::sub_agent::remote_config_parser::AgentRemoteConfigParser;
use crate::utils::thread_context::StartedThreadContext;
use crate::version_checker::k8s::checkers::spawn_version_checker;
use crate::version_checker::k8s::helmrelease::HelmReleaseVersionChecker;
use crate::{agent_control::error::AgentError, opamp::client_builder::DefaultOpAMPClientBuilder};
use crate::{
    k8s::store::K8sStore, sub_agent::k8s::builder::K8sSubAgentBuilder,
    values::k8s::ConfigRepositoryConfigMap,
};
use opamp_client::operation::settings::DescriptionValueType;
use resource_detection::system::hostname::get_hostname;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, error, info, warn};

pub const NAMESPACE_VARIABLE_NAME: &str = "namespace";
pub const NAMESPACE_AGENTS_VARIABLE_NAME: &str = "namespace_agents";

impl AgentControlRunner {
    pub(super) fn run_k8s(self) -> Result<(), AgentError> {
        info!("Starting the k8s client");
        let k8s_client = Arc::new(
            SyncK8sClient::try_new(self.runtime)
                .map_err(|e| AgentError::ExternalError(e.to_string()))?,
        );
        let k8s_store = Arc::new(K8sStore::new(
            k8s_client.clone(),
            self.k8s_config.namespace.clone(),
        ));

        debug!("Initialising yaml_config_repository");
        let yaml_config_repository = if self.opamp_http_builder.is_some() {
            Arc::new(ConfigRepositoryConfigMap::new(k8s_store.clone()).with_remote())
        } else {
            Arc::new(ConfigRepositoryConfigMap::new(k8s_store.clone()))
        };

        let config_storer = Arc::new(AgentControlConfigStore::new(yaml_config_repository.clone()));

        info!("Loading Agent Control configuration");
        let agent_control_config = config_storer.load()?;

        let fleet_id = agent_control_config
            .fleet_control
            .as_ref()
            .map(|c| c.fleet_id.to_string())
            .unwrap_or_default();

        let identifiers = get_identifiers(self.k8s_config.cluster_name.clone(), fleet_id);
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
                info!("Starting Agent Control OpAMP client");
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

        let agent_control_variables = HashMap::from([
            (
                NAMESPACE_VARIABLE_NAME.to_string(),
                Variable::new_final_string_variable(self.k8s_config.namespace.clone()),
            ),
            (
                NAMESPACE_AGENTS_VARIABLE_NAME.to_string(),
                Variable::new_final_string_variable(self.k8s_config.namespace_agents.clone()),
            ),
        ]);

        let template_renderer: TemplateRenderer<ConfigurationPersisterFile> =
            TemplateRenderer::default()
                .with_agent_control_variables(agent_control_variables.clone().into_iter());

        let mut secrets_providers = SecretsProviders::new()
            .with_env()
            .with_k8s_secret(k8s_client.clone());
        if let Some(config) = &agent_control_config.secrets_providers {
            secrets_providers = secrets_providers.with_config(config.clone()).map_err(|e| {
                AgentError::ConfigResolve(AgentControlConfigError::Load(format!(
                    "failed to load secrets providers: {e}"
                )))
            })?;
        }

        let agents_assembler = Arc::new(LocalEffectiveAgentsAssembler::new(
            self.agent_type_registry.clone(),
            template_renderer,
            self.agent_type_var_constraints,
            secrets_providers,
            &self.base_paths.remote_dir,
        ));

        let supervisor_builder =
            SupervisorBuilderK8s::new(k8s_client.clone(), self.k8s_config.clone());

        let signature_validator = Arc::new(self.signature_validator);
        let remote_config_validators = vec![
            SupportedRemoteConfigValidator::Signature(signature_validator.clone()),
            SupportedRemoteConfigValidator::Regex(RegexValidator::default()),
        ];

        let remote_config_parser = AgentRemoteConfigParser::new(remote_config_validators);

        let sub_agent_builder = K8sSubAgentBuilder::new(
            opamp_client_builder.as_ref(),
            &instance_id_getter,
            self.k8s_config.clone(),
            Arc::new(supervisor_builder),
            Arc::new(remote_config_parser),
            yaml_config_repository.clone(),
            agents_assembler,
            self.sub_agent_publisher,
        );

        let garbage_collector = K8sGarbageCollector {
            k8s_client: k8s_client.clone(),
            namespace: self.k8s_config.namespace.clone(),
            namespace_agents: self.k8s_config.namespace_agents.clone(),
            cr_type_meta: self.k8s_config.cr_type_meta,
        };

        info!("Initiating cleanup of outdated resources from previous Agent Control executions");
        // Cleanup of the existing resources managed by Agent Control but not existing in the
        // config loaded from the first time, for example from previous executions.
        garbage_collector
            .retain(K8sGarbageCollector::active_config_ids(
                &agent_control_config.dynamic.agents,
            ))
            .map_err(ResourceCleanerError::from)?;

        let registry_config_validator =
            RegistryDynamicConfigValidator::new(self.agent_type_registry);

        let dynamic_config_validator = K8sReleaseNamesConfigValidator::new(
            registry_config_validator,
            self.k8s_config.ac_release_name.to_string(),
            self.k8s_config.cd_release_name.to_string(),
        );

        // The http server stops on Drop. We need to keep it while the agent control is running.
        let _http_server = self.http_server_runner.map(Runner::start);

        let cd_remote_updates_enabled =
            self.k8s_config.cd_remote_update && !self.k8s_config.cd_release_name.is_empty();
        let ac_release_name_exists = !self.k8s_config.ac_release_name.is_empty();

        let health_checker_builder = agent_control_health_checker_builder(
            k8s_client.clone(),
            self.k8s_config.namespace.to_string(),
            ac_release_name_exists.then(|| self.k8s_config.ac_release_name.clone()),
            cd_remote_updates_enabled.then(|| self.k8s_config.cd_release_name.clone()),
        );

        let k8s_ac_updater = K8sACUpdater::new(
            self.k8s_config.ac_remote_update,
            self.k8s_config.cd_remote_update,
            k8s_client.clone(),
            self.k8s_config.namespace.clone(),
            self.k8s_config.current_chart_version.clone(),
            self.k8s_config.ac_release_name,
            self.k8s_config.cd_release_name.clone(),
        );
        let (agent_control_internal_publisher, agent_control_internal_consumer) = pub_sub();

        let _cd_version_checker = cd_remote_updates_enabled.then(|| {
            start_cd_version_checker(
                k8s_client,
                self.k8s_config.namespace.clone(),
                self.k8s_config.cd_release_name.clone(),
                agent_control_internal_publisher.clone(),
            )
        });

        AgentControl::new(
            maybe_client,
            sub_agent_builder,
            SystemTime::now(),
            config_storer,
            self.agent_control_publisher,
            self.application_event_consumer,
            maybe_opamp_consumer,
            agent_control_internal_publisher,
            agent_control_internal_consumer,
            SupportedRemoteConfigValidator::Signature(signature_validator),
            dynamic_config_validator,
            garbage_collector,
            k8s_ac_updater,
            health_checker_builder,
            agent_control_config,
        )
        .run()
    }
}

fn start_cd_version_checker(
    k8s_client: Arc<SyncK8sClient>,
    namespace: String,
    release_name: String,
    ac_internal_publisher: EventPublisher<AgentControlInternalEvent>,
) -> StartedThreadContext {
    spawn_version_checker(
        release_name.clone(),
        HelmReleaseVersionChecker::new(
            k8s_client,
            helmrelease_v2_type_meta(),
            namespace,
            release_name,
            OPAMP_CD_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
        ),
        ac_internal_publisher,
        // The below argument expects a function "AgentVersion -> T"
        // where T is the "event" sendable by the above publisher.
        // Using an enum variant that wraps a type is the same as a function taking the type.
        // Same as passing "|x| AgentControlInternalEvent::AgentControlCdVersionUpdated(x)"
        AgentControlInternalEvent::AgentControlCdVersionUpdated,
        VersionCheckerInterval::default(),
        AGENT_CONTROL_VERSION_CHECKER_INITIAL_DELAY,
    )
}

pub fn agent_control_opamp_non_identifying_attributes(
    identifiers: &Identifiers,
) -> HashMap<String, DescriptionValueType> {
    let hostname = get_hostname().unwrap_or_else(|e| {
        error!("cannot retrieve hostname: {}", e.to_string());
        "unknown_hostname".to_string()
    });

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

    if k8s_config.current_chart_version.is_empty() {
        warn!("Agent Control chart version was not set, it will not be reported");
        return attributes;
    }

    let chart_version = k8s_config.current_chart_version.to_string();

    attributes.insert(
        OPAMP_AC_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
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
            current_chart_version: "1.2.3".to_string(),
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
