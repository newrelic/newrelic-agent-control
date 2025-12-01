use super::runtime::tokio_runtime;
use actix_web::{App, HttpResponse, HttpServer, web};
use base64::Engine;
use base64::prelude::{BASE64_STANDARD, BASE64_URL_SAFE_NO_PAD};
use newrelic_agent_control::opamp::instance_id::InstanceID;
use newrelic_agent_control::opamp::remote_config::AGENT_CONFIG_PREFIX;
use newrelic_agent_control::opamp::remote_config::signature::{
    ED25519, SIGNATURE_CUSTOM_CAPABILITY, SIGNATURE_CUSTOM_MESSAGE_TYPE, SignatureFields,
};
use opamp_client::opamp::proto::{
    AgentConfigFile, AgentConfigMap, AgentDescription, AgentRemoteConfig, AgentToServer,
    ComponentHealth, CustomMessage, EffectiveConfig, RemoteConfigStatus, ServerToAgent,
    ServerToAgentFlags,
};
use opamp_client::operation::instance_uid::InstanceUid;
use prost::Message;
use ring::digest;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use serde_json::json;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Mutex;
use std::{collections::HashMap, net, sync::Arc};
use tokio::task::JoinHandle;

const FAKE_SERVER_PATH: &str = "/opamp-fake-server";
const JWKS_SERVER_PATH: &str = "/jwks";
const JWKS_PUBLIC_KEY_ID: &str = "fakeKeyName/0";

/// Represents the state of the FakeServer.
struct ServerState {
    agent_state: HashMap<InstanceID, AgentState>,
    // Key pair to sign remote configuration
    key_pair: Ed25519KeyPair,
}

#[derive(Default)]
struct AgentState {
    sequence_number: u64,
    health_status: Option<ComponentHealth>,
    attributes: AgentDescription,
    remote_config: Option<RemoteConfig>,
    effective_config: EffectiveConfig,
    config_status: RemoteConfigStatus,
}

impl ServerState {
    fn generate() -> Self {
        Self {
            agent_state: HashMap::new(),
            key_pair: generate_key_pair(),
        }
    }
}

#[derive(Clone, Debug, Default)]
/// Represents a remote configuration that can be sent to the agent.
pub struct RemoteConfig {
    raw_body: String,
    hash: String,
}

impl From<&str> for RemoteConfig {
    fn from(value: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        value.to_string().hash(&mut hasher);
        Self {
            raw_body: value.to_string(),
            hash: hasher.finish().to_string(),
        }
    }
}

/// FakeServer represents a OpAMP mock server that can be used for testing purposed.
/// The underlying http server will be aborted when the object is dropped.
pub struct FakeServer {
    handle: JoinHandle<()>,
    state: Arc<Mutex<ServerState>>,
    port: u16,
    path: String,
}

impl FakeServer {
    /// Gets the endpoint to be used in the Super-Agent static configuration.
    pub fn endpoint(&self) -> String {
        format!("http://localhost:{}{}", self.port, self.path)
    }

    pub fn jwks_endpoint(&self) -> String {
        format!("http://localhost:{}{}", self.port, JWKS_SERVER_PATH)
    }

    /// Starts and returns new FakeServer in a random port.
    pub fn start_new() -> Self {
        // While binding to port 0, the kernel gives you a free ephemeral port.
        let listener = net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let state = Arc::new(Mutex::new(ServerState::generate()));

        let handle = tokio_runtime().spawn(Self::run_http_server(listener, state.clone()));

        Self {
            handle,
            state,
            port,
            path: FAKE_SERVER_PATH.to_string(),
        }
    }

    async fn run_http_server(listener: net::TcpListener, state: Arc<Mutex<ServerState>>) {
        HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(state.clone()))
                .service(web::resource(FAKE_SERVER_PATH).to(opamp_handler))
                .service(web::resource(JWKS_SERVER_PATH).to(jwks_handler))
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
    pub fn set_config_response(&mut self, identifier: InstanceID, response: impl AsRef<str>) {
        let mut state = self.state.lock().unwrap();
        state
            .agent_state
            .entry(identifier)
            .or_default()
            .remote_config = Some(response.as_ref().into());
    }

    pub fn get_health_status(&self, identifier: &InstanceID) -> Option<ComponentHealth> {
        let state = self.state.lock().unwrap();
        state
            .agent_state
            .get(identifier)
            .and_then(|s| s.health_status.clone())
    }
    pub fn get_attributes(&self, identifier: &InstanceID) -> Option<AgentDescription> {
        let state = self.state.lock().unwrap();
        state
            .agent_state
            .get(identifier)
            .map(|s| s.attributes.clone())
    }

    pub fn get_effective_config(&self, identifier: InstanceID) -> Option<EffectiveConfig> {
        let state = self.state.lock().unwrap();
        state
            .agent_state
            .get(&identifier)
            .map(|s| s.effective_config.clone())
    }

    pub fn get_remote_config_status(&self, identifier: InstanceID) -> Option<RemoteConfigStatus> {
        let state = self.state.lock().unwrap();
        state
            .agent_state
            .get(&identifier)
            .map(|s| s.config_status.clone())
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

async fn opamp_handler(state: web::Data<Arc<Mutex<ServerState>>>, req: web::Bytes) -> HttpResponse {
    let message = AgentToServer::decode(req).unwrap();
    let identifier: InstanceID = InstanceUid::try_from(message.clone().instance_uid)
        .unwrap()
        .into();

    let mut server_state = state.lock().unwrap();

    let agent_state = server_state
        .agent_state
        .entry(identifier.clone())
        .or_default();

    // Check sequence number
    let mut flags = ServerToAgentFlags::Unspecified as u64;
    if message.sequence_num == (agent_state.sequence_number + 1) {
        // case 1: first opamp connection start with seq number 1
        // case 2: Any valid new sequence number
        agent_state.sequence_number += 1;
    } else {
        flags = ServerToAgentFlags::ReportFullState as u64;
        // upon report full state the opamp client will send a new AgentToServer
        // increasing the seq number so current should be the valid
        agent_state.sequence_number = message.sequence_num;
    }

    if let Some(health) = message.health {
        agent_state.health_status = Some(health);
    }

    if let Some(attributes) = message.agent_description {
        agent_state.attributes = attributes;
    }

    if let Some(effective_cfg) = message.effective_config {
        agent_state.effective_config = effective_cfg;
    }

    // Process config status:
    // Stop sending the RemoteConfig once we got a RemoteConfigStatus response associated with the hash.
    // emulating what FC currently does.
    if let Some(cfg_status) = message.remote_config_status {
        if agent_state
            .remote_config
            .as_ref()
            .is_some_and(|config_response| {
                config_response.hash.encode_to_vec() == cfg_status.last_remote_config_hash
            })
        {
            agent_state.remote_config = None;
        }
        agent_state.config_status = cfg_status;
    }

    let remote_config = agent_state.remote_config.clone();

    let _ = agent_state; // We need to get rid of the mutable reference before leveraging another immutable.

    let server_to_agent = build_response(identifier, remote_config, &server_state.key_pair, flags);
    HttpResponse::Ok().body(server_to_agent)
}

async fn jwks_handler(state: web::Data<Arc<Mutex<ServerState>>>, _req: web::Bytes) -> HttpResponse {
    let server_state = state.lock().unwrap();
    let public_key = server_state.key_pair.public_key().as_ref().to_vec();
    let enc_public_key = BASE64_URL_SAFE_NO_PAD.encode(&public_key);
    let payload = json!({
        "keys": [
            {
                "kty": "OKP",
                "alg": null,
                "use": "sig",
                "kid": JWKS_PUBLIC_KEY_ID,
                "n": null,
                "x": enc_public_key,
                "y": null,
                "crv": "Ed25519"
            }
        ]
    });
    HttpResponse::Ok().json(payload)
}

fn build_response(
    instance_id: InstanceID,
    agent_remote_config: Option<RemoteConfig>,
    key_pair: &Ed25519KeyPair,
    flags: u64,
) -> Vec<u8> {
    let mut remote_config = None;
    let mut custom_message = None;

    if let Some(config) = agent_remote_config {
        remote_config = Some(AgentRemoteConfig {
            config_hash: config.hash.encode_to_vec(),
            config: Some(AgentConfigMap {
                config_map: HashMap::from([(
                    AGENT_CONFIG_PREFIX.to_string(),
                    AgentConfigFile {
                        body: config.raw_body.clone().into_bytes(),
                        content_type: " text/yaml".to_string(),
                    },
                )]),
            }),
        });

        // Actual implementation from FC side signs the Base64 representation of the SHA256 digest
        // of the message (i.e. the remote configs). Hence, to verify the signature, we need to
        // compute the SHA256 digest of the message, then Base64 encode it, and finally verify
        // the signature against that.
        let digest = digest::digest(&digest::SHA256, config.raw_body.as_bytes());
        let msg = BASE64_STANDARD.encode(digest);
        let signature = key_pair.sign(msg.as_bytes());

        let custom_message_data = HashMap::from([(
            AGENT_CONFIG_PREFIX.to_string(),
            vec![SignatureFields {
                signature: BASE64_STANDARD.encode(signature),
                signing_algorithm: ED25519,
                key_id: JWKS_PUBLIC_KEY_ID.to_string(),
            }],
        )]);

        custom_message = Some(CustomMessage {
            capability: SIGNATURE_CUSTOM_CAPABILITY.to_string(),
            r#type: SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
            data: serde_json::to_vec(&custom_message_data).unwrap(),
        });
    }
    ServerToAgent {
        instance_uid: instance_id.into(),
        remote_config,
        custom_message,
        flags,
        ..Default::default()
    }
    .encode_to_vec()
}

fn generate_key_pair() -> Ed25519KeyPair {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
}
