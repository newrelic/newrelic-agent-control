#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::opamp::instance_id::getter::InstanceIDWithIdentifiersGetter;
use crate::opamp::instance_id::Identifiers;
use crate::opamp::operations::build_opamp_with_channel;
use crate::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use crate::sub_agent::event_processor_builder::EventProcessorBuilder;
use crate::super_agent::config::AgentID;
use crate::super_agent::config_storer::loader_storer::SuperAgentConfigLoader;
use crate::super_agent::defaults::{FLEET_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY};
use crate::super_agent::run::SuperAgentRunner;
use crate::super_agent::{super_agent_fqn, SuperAgent};
use crate::{
    k8s::{garbage_collector::NotStartedK8sGarbageCollector, store::K8sStore},
    sub_agent::k8s::builder::K8sSubAgentBuilder,
    values::k8s::ValuesRepositoryConfigMap,
};
use crate::{
    opamp::{hash_repository::k8s::HashRepositoryConfigMap, instance_id},
    super_agent::{config_storer::file::SuperAgentConfigStore, error::AgentError},
};
use opamp_client::operation::settings::DescriptionValueType;
use resource_detection::system::hostname::HostnameGetter;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info};

impl SuperAgentRunner {
    pub fn run_super_agent(self) -> Result<(), AgentError> {
        info!("Starting the k8s client");
        let k8s_client = Arc::new(
            SyncK8sClient::try_new(
                self.runtime,
                self.k8s_config.namespace.clone(),
                self.k8s_config.cr_type_meta.clone(),
            )
            .map_err(|e| AgentError::ExternalError(e.to_string()))?,
        );
        let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));

        let mut vr_super_agent = ValuesRepositoryConfigMap::new(k8s_store.clone());
        let mut vr_sub_agent = ValuesRepositoryConfigMap::new(k8s_store.clone());
        if self.opamp_client_builder.is_some() {
            vr_super_agent = vr_super_agent.with_remote();
            vr_sub_agent = vr_sub_agent.with_remote();
        }
        let vr_sub_agent = Arc::new(vr_sub_agent);
        let vr_super_agent = Arc::new(vr_super_agent);

        let config_storer = Arc::new(SuperAgentConfigStore::new(vr_super_agent));
        let config = config_storer.load()?;

        let identifiers =
            instance_id::get_identifiers(self.k8s_config.cluster_name.clone(), config.fleet_id);
        info!("Instance Identifiers: {}", identifiers);

        let mut non_identifying_attributes =
            super_agent_opamp_non_identifying_attributes(&identifiers);
        non_identifying_attributes.insert(
            "cluster.name".to_string(),
            self.k8s_config.cluster_name.clone().into(),
        );

        let instance_id_getter = InstanceIDWithIdentifiersGetter::new_k8s_instance_id_getter(
            k8s_store.clone(),
            identifiers,
        );

        let agents_assembler = LocalEffectiveAgentsAssembler::new(vr_sub_agent.clone());
        let hash_repository = Arc::new(HashRepositoryConfigMap::new(k8s_store));
        let sub_agent_event_processor_builder =
            EventProcessorBuilder::new(hash_repository.clone(), vr_sub_agent);

        info!("Creating the k8s sub_agent builder");
        let sub_agent_builder = K8sSubAgentBuilder::new(
            self.opamp_client_builder.as_ref(),
            &instance_id_getter,
            k8s_client.clone(),
            hash_repository.clone(),
            &agents_assembler,
            &sub_agent_event_processor_builder,
            self.k8s_config.clone(),
        );

        let (maybe_client, opamp_consumer) = self
            .opamp_client_builder
            .as_ref()
            .map(|builder| {
                build_opamp_with_channel(
                    builder,
                    &instance_id_getter,
                    AgentID::new_super_agent_id(),
                    &super_agent_fqn(),
                    non_identifying_attributes,
                )
            })
            // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
            .transpose()?
            .map(|(client, consumer)| (Some(client), Some(consumer)))
            .unwrap_or_default();

        let gcc = NotStartedK8sGarbageCollector::new(config_storer.clone(), k8s_client);
        let _started_gcc = gcc.start();

        SuperAgent::new(
            maybe_client,
            hash_repository,
            sub_agent_builder,
            config_storer,
            self.super_agent_publisher,
        )
        .run(self.application_event_consumer, opamp_consumer)
    }
}

pub fn super_agent_opamp_non_identifying_attributes(
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
            HOST_NAME_ATTRIBUTE_KEY().to_string(),
            DescriptionValueType::String(hostname),
        ),
        (
            FLEET_ID_ATTRIBUTE_KEY().to_string(),
            DescriptionValueType::String(identifiers.fleet_id.clone()),
        ),
    ])
}
