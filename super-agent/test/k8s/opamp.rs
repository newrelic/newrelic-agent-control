use crate::common::{
    block_on, check_deployments_exist, create_mock_config_maps, tokio_runtime,
    MockOpAMPClientBuilderMock, MockStartedOpAMPClientMock,
};
use kube::Client;
use newrelic_super_agent::k8s::store::STORE_KEY_LOCAL_DATA_CONFIG;
use newrelic_super_agent::opamp::callbacks::AgentCallbacks;
use newrelic_super_agent::opamp::instance_id;
use newrelic_super_agent::opamp::remote_config_publisher::OpAMPRemoteConfigPublisher;
use newrelic_super_agent::super_agent::config_storer::storer::SuperAgentConfigLoader;
use newrelic_super_agent::{
    agent_type::{agent_type_registry::LocalRegistry, renderer::TemplateRenderer},
    event::{
        channel::pub_sub, channel::EventConsumer, channel::EventPublisher, OpAMPEvent,
        SuperAgentEvent,
    },
    k8s::{client::SyncK8sClient, store::K8sStore},
    opamp::{
        hash_repository::HashRepositoryConfigMap,
        instance_id::{getter::ULIDInstanceIDGetter, Storer},
        operations::build_opamp_and_start_client,
        remote_config::{ConfigMap, RemoteConfig},
        remote_config_hash::Hash,
    },
    sub_agent::{
        effective_agents_assembler::LocalEffectiveAgentsAssembler,
        event_processor_builder::EventProcessorBuilder, k8s::builder::K8sSubAgentBuilder,
        persister::config_persister_file::ConfigurationPersisterFile,
        values::ValuesRepositoryConfigMap,
    },
    super_agent::{
        config::{AgentID, K8sConfig},
        config_storer::SuperAgentConfigStoreFile,
        super_agent_fqn, SuperAgent,
    },
};
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::{
    collections::HashMap,
    io::Write,
    sync::{Arc, Mutex},
    thread::sleep as thread_sleep,
    thread::spawn,
    time::Duration,
};
use tempfile::NamedTempFile;

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_opamp_add_sub_agent() {
    let k8s_ns = "default";
    // We need to create this (raw) client in order to query the K8s API.
    let client = block_on(async { Client::try_default().await.unwrap() });
    // We need to create it outside to avoid NamedTempFile being dropped.
    // TODO: Temporary workaround waiting on the super-agent config to be persisted on a config map.
    let super_agent_config = create_super_agent_config();

    // Set OpAMP client builders mock expectations.
    let super_agent_expectations = vec![OpAMPExpectation {
        agent_id: "super-agent".to_string(),
        health_calls: 2,
        status_calls: 2,
    }];
    let sub_agent_expectations = vec![
        OpAMPExpectation {
            agent_id: "open-telemetry".to_string(),
            health_calls: 1,
            status_calls: 0,
        },
        OpAMPExpectation {
            agent_id: "open-telemetry-2".to_string(),
            health_calls: 1,
            status_calls: 0,
        },
    ];

    let test_env = K8sOpAMPEnv::new(
        super_agent_config.path().to_path_buf(),
        k8s_ns,
        super_agent_expectations,
        sub_agent_expectations,
    );

    // Create config map for the sub agent defined in the initial config.
    block_on(create_mock_config_maps(
        client.clone(),
        k8s_ns,
        "local-data-open-telemetry",
        STORE_KEY_LOCAL_DATA_CONFIG,
    ));

    // Retrieve needed values before run_super_agent consumes itself.
    let opamp_publisher = test_env.opamp_publisher.clone();
    let super_agent_publisher = test_env.super_agent_publisher.clone();

    let running_agent = test_env.run_super_agent();

    let remote_config = RemoteConfig::new(
        AgentID::new_super_agent_id(),
        Hash::new("a-hash".to_string()),
        Some(ConfigMap::new(HashMap::from([(
            "".to_string(),
            r#"
agents:
  open-telemetry:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
  open-telemetry-2:
    agent_type: "newrelic/io.opentelemetry.collector:0.1.0"
"#
            .to_string(),
        )]))),
    );

    // Create config map for the new added sub agent.
    // In a typical scenario, when a new agent is added remotely, its configuration would also be
    // expected to come from a remote source. Here, for the purposes of testing, we manually
    // create a mock config map locally to simulate the presence of agent configuration.
    block_on(create_mock_config_maps(
        client.clone(),
        k8s_ns,
        "local-data-open-telemetry-2",
        STORE_KEY_LOCAL_DATA_CONFIG,
    ));

    // Wait some time to let the super agent to be up.
    thread_sleep(Duration::from_millis(500));

    opamp_publisher
        .publish(OpAMPEvent::RemoteConfigReceived(remote_config))
        .unwrap();

    // Wait some time to let the (sub)agents to be created.
    thread_sleep(Duration::from_millis(5000));

    block_on(check_deployments_exist(
        client,
        &[
            "open-telemetry-opentelemetry-collector",
            "open-telemetry-2-opentelemetry-collector",
        ],
        k8s_ns,
        20,
        Duration::from_millis(1500),
    ));

    super_agent_publisher
        .publish(SuperAgentEvent::StopRequested)
        .unwrap();

    assert!(running_agent.join().is_ok());
}

///////////////////////////////////////////
////// K8s OpAMP Environment Setup ////////
///////////////////////////////////////////
struct OpAMPExpectation {
    agent_id: String,
    health_calls: usize,
    status_calls: usize,
}

struct K8sOpAMPEnv {
    k8s_client: Arc<SyncK8sClient>,
    k8s_config: K8sConfig,
    instance_id_getter: ULIDInstanceIDGetter<Storer>,
    hash_repository: Arc<HashRepositoryConfigMap>,
    config_storer: Arc<SuperAgentConfigStoreFile>,
    opamp_publisher: EventPublisher<OpAMPEvent>,
    opamp_consumer: EventConsumer<OpAMPEvent>,
    super_agent_publisher: EventPublisher<SuperAgentEvent>,
    super_agent_consumer: EventConsumer<SuperAgentEvent>,
    super_agent_opamp_builder:
        MockOpAMPClientBuilderMock<AgentCallbacks<OpAMPRemoteConfigPublisher>>,
    sub_agent_opamp_builder: MockOpAMPClientBuilderMock<AgentCallbacks<OpAMPRemoteConfigPublisher>>,
    sub_agent_event_processor_builder:
        EventProcessorBuilder<HashRepositoryConfigMap, ValuesRepositoryConfigMap>,
    agents_assembler: LocalEffectiveAgentsAssembler<
        LocalRegistry,
        ValuesRepositoryConfigMap,
        TemplateRenderer<ConfigurationPersisterFile>,
    >,
}

impl K8sOpAMPEnv {
    // The new method follows the same setup process as the main function, preparing all necessary components up to the point of running the super agent.
    // Ideally, if the run_super_agent function were located in its own module rather than the main, we could leverage it.
    fn new(
        config_file: PathBuf,
        namespace: &str,
        super_agent_expectations: Vec<OpAMPExpectation>,
        sub_agent_expectations: Vec<OpAMPExpectation>,
    ) -> Self {
        let super_agent_config_storer =
            SuperAgentConfigStoreFile::new(config_file.as_path()).with_remote();

        let (k8s_config, k8s_client, k8s_store, instance_id_getter, hash_repository) =
            Self::setup_environment(namespace.to_string(), &super_agent_config_storer);

        let config_storer = Arc::new(super_agent_config_storer);

        let (opamp_publisher, opamp_consumer) = pub_sub();
        let (super_agent_publisher, super_agent_consumer) = pub_sub();

        let vr = ValuesRepositoryConfigMap::new(k8s_store.clone()).with_remote();
        let values_repository = Arc::new(vr);

        let agents_assembler = LocalEffectiveAgentsAssembler::new(values_repository.clone());

        let sub_agent_event_processor_builder =
            EventProcessorBuilder::new(hash_repository.clone(), values_repository.clone());

        // Set up mock expectations.
        ///////////////////////////
        let (super_agent_builder, _super_agent_publishers) =
            Self::setup_opamp_client_builder_mock(super_agent_expectations);

        let (sub_agent_builder, _sub_agent_publishers) =
            Self::setup_opamp_client_builder_mock(sub_agent_expectations);
        ///////////////////////////

        Self {
            k8s_client,
            k8s_config,
            instance_id_getter,
            hash_repository,
            config_storer,
            agents_assembler,
            opamp_publisher,
            opamp_consumer,
            super_agent_publisher,
            super_agent_consumer,
            super_agent_opamp_builder: super_agent_builder,
            sub_agent_opamp_builder: sub_agent_builder,
            sub_agent_event_processor_builder,
        }
    }

    fn setup_environment(
        test_ns: String,
        storer: &SuperAgentConfigStoreFile,
    ) -> (
        K8sConfig,
        Arc<SyncK8sClient>,
        Arc<K8sStore>,
        ULIDInstanceIDGetter<Storer>,
        Arc<HashRepositoryConfigMap>,
    ) {
        let k8s_config_result = SuperAgentConfigLoader::load(storer);
        let k8s_config = match k8s_config_result {
            Ok(config) => config.k8s.expect("K8s configuration should be present"),
            Err(e) => panic!("Failed to load K8s configuration: {:?}", e),
        };

        let k8s_client = Arc::new(
            SyncK8sClient::try_new_with_reflectors(
                tokio_runtime(),
                test_ns,
                k8s_config.cr_type_meta.clone(),
            )
            .expect("Failed to create K8s client"),
        );
        let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));
        let hash_repository = Arc::new(HashRepositoryConfigMap::new(k8s_store.clone()));
        let identifiers = instance_id::get_identifiers(k8s_config.cluster_name.clone());
        let instance_id_getter =
            ULIDInstanceIDGetter::try_with_identifiers(k8s_store.clone(), identifiers)
                .expect("instance id getter");

        (
            k8s_config,
            k8s_client,
            k8s_store,
            instance_id_getter,
            hash_repository,
        )
    }

    pub fn run_super_agent(self) -> JoinHandle<()> {
        let super_agent_opamp_publisher_clone = self.opamp_publisher.clone();
        spawn(move || {
            let sub_agent_builder = K8sSubAgentBuilder::new(
                Some(&self.sub_agent_opamp_builder),
                &self.instance_id_getter,
                self.k8s_client.clone(),
                self.hash_repository.clone(),
                &self.agents_assembler,
                &self.sub_agent_event_processor_builder,
                self.k8s_config.clone(),
            );

            let maybe_client = build_opamp_and_start_client(
                super_agent_opamp_publisher_clone,
                Some(&self.super_agent_opamp_builder),
                &self.instance_id_getter,
                AgentID::new_super_agent_id(),
                &super_agent_fqn(),
                HashMap::from([(
                    "cluster.name".to_string(),
                    self.k8s_config.clone().cluster_name.into(),
                )]),
            )
            .expect("Failed to build and start opamp client");

            let super_agent = SuperAgent::new(
                maybe_client,
                self.hash_repository.clone(),
                sub_agent_builder,
                self.config_storer.clone(),
            );

            super_agent
                .run(self.super_agent_consumer, self.opamp_consumer)
                .expect("Failed to run super agent");
        })
    }

    fn setup_opamp_client_builder_mock(
        expectations: Vec<OpAMPExpectation>,
    ) -> (
        MockOpAMPClientBuilderMock<AgentCallbacks<OpAMPRemoteConfigPublisher>>,
        Arc<Mutex<Vec<EventPublisher<OpAMPEvent>>>>,
    ) {
        let mut builder = MockOpAMPClientBuilderMock::new();
        // Arc<Mutex<_>> is used to safely share and modify publishers across threads and closures.
        let publishers = Arc::new(Mutex::new(Vec::new()));

        for expectation in expectations {
            let agent_id_owned = expectation.agent_id.to_string();
            let publishers_clone = Arc::clone(&publishers);

            builder
                .expect_build_and_start()
                .withf(move |_, agent_id, _| agent_id.to_string() == agent_id_owned)
                .once()
                .returning(move |opamp_publisher, _, _| {
                    let mut publishers_lock = publishers_clone.lock().unwrap();
                    publishers_lock.push(opamp_publisher);
                    let mut started_client = MockStartedOpAMPClientMock::new();
                    started_client.should_set_health(expectation.health_calls);
                    started_client.should_set_any_remote_config_status(expectation.status_calls);
                    started_client
                        .expect_stop()
                        .times(0..=1)
                        .returning(|| Ok(()));
                    Ok(started_client)
                });
        }
        (builder, publishers)
    }
}

// TODO: remove it when we leverage configMaps to store the super agent config.
// create_super_agent_config create a temporary file and writes a predefined super
// agent configuration to it. The file is automatically deleted when it goes out of scope,
// ensuring no leftover files from test runs.
pub fn create_super_agent_config() -> NamedTempFile {
    let mut temp_file = NamedTempFile::new().expect("Failed to create a temporary file");
    let local_config = r#"
opamp:
  endpoint: https://opamp.staging-service.newrelic.com/v1/opamp
  headers:
    api-key: test-api-key
k8s:
  namespace: default
  cluster_name: minikube
agents:
  open-telemetry:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
"#;
    write!(temp_file, "{}", local_config).unwrap();
    temp_file
}
