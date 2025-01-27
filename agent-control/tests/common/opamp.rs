use super::runtime::tokio_runtime;
use actix_web::{web, App, HttpResponse, HttpServer};
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use newrelic_agent_control::opamp::instance_id::InstanceID;
use newrelic_agent_control::opamp::remote_config::signature::{
    SignatureFields, ED25519, SIGNATURE_CUSTOM_CAPABILITY, SIGNATURE_CUSTOM_MESSAGE_TYPE,
};
use newrelic_agent_control::opamp::remote_config::validators::signature::public_key_fingerprint;
use opamp_client::opamp;
use prost::Message;
use rcgen::{CertificateParams, KeyPair, PKCS_ED25519};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use std::{collections::HashMap, net, sync::Arc};
use tempfile::TempDir;
use tokio::task::JoinHandle;

const FAKE_SERVER_PATH: &str = "/opamp-fake-server";
const CERT_FILE: &str = "server.crt";

pub type ConfigResponses = HashMap<InstanceID, ConfigResponse>;

/// It stores the latest received health status in the format of `ComponentHealth` for each
/// instance id.
pub type HealthStatuses = HashMap<InstanceID, opamp::proto::ComponentHealth>;
/// It stores the latest received attributes in the format of `AgentDescription` for each
/// instance id.
pub type Attributes = HashMap<InstanceID, opamp::proto::AgentDescription>;

/// It stores the latest received effective configs in the format of `EffectiveConfig` for each
/// instance id.
pub type EffectiveConfigs = HashMap<InstanceID, opamp::proto::EffectiveConfig>;

/// It stores the latest received effective configs status in the format of `RemoteConfigStatus` for each
/// instance id.
pub type RemoteConfigStatus = HashMap<InstanceID, opamp::proto::RemoteConfigStatus>;

/// Represents the state of the FakeServer.
struct State {
    health_statuses: HealthStatuses,
    attributes: Attributes,
    config_responses: ConfigResponses,
    effective_configs: EffectiveConfigs,
    config_status: RemoteConfigStatus,
    // Server private key to sign the remote config
    key_pair: KeyPair,
}
impl State {
    fn new(key_pair: KeyPair) -> Self {
        Self {
            health_statuses: Default::default(),
            attributes: Default::default(),
            config_responses: Default::default(),
            effective_configs: Default::default(),
            config_status: Default::default(),
            key_pair,
        }
    }
}

#[derive(Clone, Debug, Default)]
/// Configuration response to be returned by the server until the agent informs it is applied.
pub struct ConfigResponse {
    raw_body: Option<String>,
}

impl From<&str> for ConfigResponse {
    fn from(value: &str) -> Self {
        Self {
            raw_body: Some(value.to_string()),
        }
    }
}

impl ConfigResponse {
    fn encode(&self, key_pair: &KeyPair) -> Vec<u8> {
        let mut remote_config = None;
        let mut custom_message = None;

        if let Some(config) = self.raw_body.clone() {
            remote_config = Some(opamp::proto::AgentRemoteConfig {
                config_hash: "hash".into(), // fake has for the shake of simplicity
                config: Some(opamp::proto::AgentConfigMap {
                    config_map: HashMap::from([(
                        "".to_string(),
                        opamp::proto::AgentConfigFile {
                            body: config.clone().into_bytes(),
                            content_type: " text/yaml".to_string(),
                        },
                    )]),
                }),
            });

            let key_pair_ring =
                ring::signature::Ed25519KeyPair::from_pkcs8(&key_pair.serialize_der()).unwrap();
            let signature = key_pair_ring.sign(config.as_bytes());

            let custom_message_data = HashMap::from([(
                "fakeCRC".to_string(),
                vec![SignatureFields {
                    signature: BASE64_STANDARD.encode(signature.as_ref()),
                    signing_algorithm: ED25519,
                    key_id: public_key_fingerprint(&key_pair.public_key_der()),
                }],
            )]);

            custom_message = Some(opamp::proto::CustomMessage {
                capability: SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                r#type: SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                data: serde_json::to_vec(&custom_message_data).unwrap(),
            });
        }
        opamp::proto::ServerToAgent {
            instance_uid: "test".into(), // fake uid for the sake of simplicity
            remote_config,
            custom_message,
            ..Default::default()
        }
        .encode_to_vec()
    }
}

/// FakeServer represents a OpAMP mock server that can be used for testing purposed.
/// The underlying http server will be aborted when the object is dropped.
pub struct FakeServer {
    handle: JoinHandle<()>,
    state: Arc<Mutex<State>>,
    port: u16,
    path: String,
    cert_tmp_dir: TempDir,
}

impl FakeServer {
    /// Gets the endpoint to be used in the Super-Agent static configuration.
    pub fn endpoint(&self) -> String {
        format!("http://localhost:{}{}", self.port, self.path)
    }

    /// Starts and returns new FakeServer in a random port.
    pub fn start_new() -> Self {
        // While binding to port 0, the kernel gives you a free ephemeral port.
        let listener = net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let key_pair = KeyPair::generate_for(&PKCS_ED25519).unwrap();
        let cert = CertificateParams::new(vec!["localhost".to_string()])
            .unwrap()
            .self_signed(&key_pair)
            .unwrap();

        let tmp_dir = tempfile::tempdir().unwrap();
        std::fs::write(tmp_dir.path().join(CERT_FILE), cert.pem()).unwrap();

        let state = Arc::new(Mutex::new(State::new(key_pair)));

        let handle = tokio_runtime().spawn(Self::run_http_server(listener, state.clone()));

        Self {
            handle,
            state,
            port,
            path: FAKE_SERVER_PATH.to_string(),
            cert_tmp_dir: tmp_dir,
        }
    }

    async fn run_http_server(listener: net::TcpListener, state: Arc<Mutex<State>>) {
        HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(state.clone()))
                .service(web::resource(FAKE_SERVER_PATH).to(opamp_handler))
        })
        .listen(listener)
        .unwrap_or_else(|err| panic!("Could not bind the HTTP server to the listener: {err}"))
        .run()
        .await
        .unwrap_or_else(|err| panic!("Failed to run the HTTP server: {err}"))
    }

    /// Sets a response for the provided identifier. If a response already existed, it is overwritten.
    /// It will be returned by the server until the agent informs that the remote configuration has been applied,
    /// then the server will return a `None` (no-changes) configuration in following requests.
    /// The identifier should be a valid UUID.
    pub fn set_config_response(&mut self, identifier: InstanceID, response: ConfigResponse) {
        let mut state = self.state.lock().unwrap();
        state.config_responses.insert(identifier, response);
    }

    pub fn cert_file_path(&self) -> PathBuf {
        self.cert_tmp_dir.path().join(CERT_FILE)
    }

    pub fn get_health_status(
        &self,
        identifier: &InstanceID,
    ) -> Option<opamp::proto::ComponentHealth> {
        let state = self.state.lock().unwrap();
        state.health_statuses.get(identifier).cloned()
    }
    pub fn get_attributes(
        &self,
        identifier: &InstanceID,
    ) -> Option<opamp::proto::AgentDescription> {
        let state = self.state.lock().unwrap();
        state.attributes.get(identifier).cloned()
    }

    pub fn get_effective_config(
        &self,
        identifier: InstanceID,
    ) -> Option<opamp::proto::EffectiveConfig> {
        let state = self.state.lock().unwrap();
        state.effective_configs.get(&identifier).cloned()
    }

    #[allow(dead_code)] // used only for onhost
    pub fn get_remote_config_status(
        &self,
        identifier: InstanceID,
    ) -> Option<opamp::proto::RemoteConfigStatus> {
        let state = self.state.lock().unwrap();
        state.config_status.get(&identifier).cloned()
    }

    fn stop(&self) {
        self.handle.abort();
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn opamp_handler(state: web::Data<Arc<Mutex<State>>>, req: web::Bytes) -> HttpResponse {
    tokio::time::sleep(Duration::from_secs(1)).await;
    let message = opamp::proto::AgentToServer::decode(req).unwrap();
    let instance_id = InstanceID::try_from(message.clone().instance_uid).unwrap();

    // Store the health status
    if let Some(health) = message.clone().health {
        let mut state = state.lock().unwrap();
        state.health_statuses.insert(instance_id.clone(), health);
    }

    // Store the attributes
    if let Some(attributes) = message.clone().agent_description {
        let mut state = state.lock().unwrap();
        state.attributes.insert(instance_id.clone(), attributes);
    }

    // Store the effective config
    if let Some(effective_cfg) = message.clone().effective_config {
        let mut state = state.lock().unwrap();
        state
            .effective_configs
            .insert(instance_id.clone(), effective_cfg);
    }

    // Store the remote config status
    if let Some(cfg_status) = message.clone().remote_config_status {
        let mut state = state.lock().unwrap();
        state.config_status.insert(instance_id.clone(), cfg_status);
    }

    let mut state = state.lock().unwrap();

    let config_response = state
        .config_responses
        .get(&instance_id)
        .map(|config_response| config_response.to_owned())
        .unwrap_or_default();

    // Just return once each response
    // If we needed to test "retries" (sending the same response more than once if we don't get the expected
    // AgentToServer message. Ex: the RemoteConfigStatus(Applying) is not received) we would need to check the
    // `message` content before removing the config response from the state.
    state.config_responses.remove(&instance_id);

    HttpResponse::Ok().body(config_response.encode(&state.key_pair))
}
