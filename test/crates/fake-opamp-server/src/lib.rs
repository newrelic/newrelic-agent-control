use actix_web::{App, HttpResponse, HttpServer, web};
use aws_lc_rs::digest;
use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair as _};
use base64::Engine;
use base64::prelude::{BASE64_STANDARD, BASE64_URL_SAFE_NO_PAD};
use opamp_client::opamp::proto::any_value::Value;
use opamp_client::opamp::proto::{
    AgentConfigFile, AgentConfigMap, AgentDescription, AgentRemoteConfig, AgentToServer,
    ComponentHealth, CustomMessage, EffectiveConfig, RemoteConfigStatus, RemoteConfigStatuses,
    ServerToAgent, ServerToAgentFlags,
};
use opamp_client::operation::instance_uid::InstanceUid;
use prost::Message;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::net;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

pub use opamp_client::operation::instance_uid::InstanceUid as InstanceID;

pub const FAKE_SERVER_PATH: &str = "/opamp-fake-server";
pub const JWKS_SERVER_PATH: &str = "/jwks";
const JWKS_PUBLIC_KEY_ID: &str = "fakeKeyName/0";
const AGENT_CONFIG_PREFIX: &str = "agentConfig";
const SIGNATURE_CUSTOM_CAPABILITY: &str = "com.newrelic.security.configSignature";
const SIGNATURE_CUSTOM_MESSAGE_TYPE: &str = "newrelicRemoteConfigSignature";
const ED25519_ALG: &str = "ED25519";

struct ServerState {
    agent_state: HashMap<InstanceUid, AgentState>,
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

    /// Sets the pending remote config for the given agent, overwriting any previous one.
    fn set_multi_config(&mut self, identifier: InstanceUid, config_map: HashMap<String, String>) {
        self.agent_state
            .entry(identifier)
            .or_default()
            .remote_config = Some(RemoteConfig::new(config_map));
    }
}

/// Represents a remote configuration that can be sent to the agent.
#[derive(Clone, Debug, Default)]
pub struct RemoteConfig(AgentRemoteConfig);

impl RemoteConfig {
    pub fn new(config_map: HashMap<String, String>) -> Self {
        let built_map: HashMap<String, AgentConfigFile> = config_map
            .into_iter()
            .map(|(key, body)| {
                (
                    key,
                    AgentConfigFile {
                        body: body.as_bytes().to_vec(),
                        content_type: "text/yaml".to_string(),
                    },
                )
            })
            .collect();

        let config_map = AgentConfigMap {
            config_map: built_map,
        };

        Self(AgentRemoteConfig {
            config_hash: Self::compute_hash(&config_map.config_map),
            config: Some(config_map),
        })
    }

    // Do not assume this replicates FC hashing.
    fn compute_hash(map: &HashMap<String, AgentConfigFile>) -> Vec<u8> {
        let mut hasher = DefaultHasher::new();
        for agent_config_file in map.values() {
            agent_config_file.body.hash(&mut hasher);
        }
        hasher.finish().to_string().into_bytes()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignatureFields {
    signature: String,
    signing_algorithm: String,
    key_id: String,
}

/// Represents a remote configuration signature custom message.
#[derive(Clone, Debug, Default)]
pub struct RemoteConfigSignature(CustomMessage);

impl RemoteConfigSignature {
    pub fn new(key_pair: &Ed25519KeyPair, remote_config: RemoteConfig) -> Self {
        let mut custom_message_data: HashMap<String, Vec<SignatureFields>> = HashMap::new();

        let config_map = remote_config.0.config.unwrap_or_default().config_map;

        for (cfg_key, cfg_content) in config_map {
            // FC signs the Base64 representation of the SHA256 digest of the message body.
            let digest = digest::digest(&digest::SHA256, &cfg_content.body);
            let msg = BASE64_STANDARD.encode(digest);
            let signature = key_pair.sign(msg.as_bytes());

            custom_message_data.insert(
                cfg_key,
                vec![SignatureFields {
                    signature: BASE64_STANDARD.encode(signature),
                    signing_algorithm: ED25519_ALG.to_string(),
                    key_id: JWKS_PUBLIC_KEY_ID.to_string(),
                }],
            );
        }

        Self(CustomMessage {
            capability: SIGNATURE_CUSTOM_CAPABILITY.to_string(),
            r#type: SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
            data: serde_json::to_vec(&custom_message_data).unwrap(),
        })
    }
}

/// OpAMP mock server for testing. The underlying HTTP server is aborted when dropped.
pub struct FakeServer {
    handle: JoinHandle<()>,
    state: Arc<Mutex<ServerState>>,
    port: u16,
    path: String,
}

impl FakeServer {
    /// Starts the server on a random port, spawning the HTTP task on the provided runtime handle.
    pub fn start(handle: &tokio::runtime::Handle) -> Self {
        let listener = net::TcpListener::bind("0.0.0.0:0").unwrap();
        Self::start_with_listener(listener, handle)
    }

    /// Starts the server on the given (already-bound) listener, spawning the HTTP task on the
    /// provided runtime handle. Useful when the caller needs to choose the bind address (e.g. the
    /// standalone binary).
    pub fn start_with_listener(
        listener: net::TcpListener,
        handle: &tokio::runtime::Handle,
    ) -> Self {
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(ServerState::generate()));
        let join_handle = handle.spawn(Self::run_http_server(listener, state.clone()));

        Self {
            handle: join_handle,
            state,
            port,
            path: FAKE_SERVER_PATH.to_string(),
        }
    }

    pub fn endpoint(&self) -> String {
        format!("http://localhost:{}{}", self.port, self.path)
    }

    pub fn jwks_endpoint(&self) -> String {
        format!("http://localhost:{}{}", self.port, JWKS_SERVER_PATH)
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

    /// Sets the remote config for the given agent. Overwrites any existing config.
    /// The server stops sending it once the agent acknowledges the hash.
    pub fn set_config_response(
        &mut self,
        identifier: impl Into<InstanceUid>,
        response: impl AsRef<str>,
    ) {
        self.state.lock().unwrap().set_multi_config(
            identifier.into(),
            HashMap::from([(
                AGENT_CONFIG_PREFIX.to_string(),
                response.as_ref().to_string(),
            )]),
        );
    }

    /// Same as `set_config_response` but accepts multiple config keys.
    pub fn set_multi_config_response(
        &mut self,
        identifier: impl Into<InstanceUid>,
        config_map: HashMap<String, String>,
    ) {
        self.state
            .lock()
            .unwrap()
            .set_multi_config(identifier.into(), config_map);
    }

    pub fn get_health_status(&self, identifier: impl Into<InstanceUid>) -> Option<ComponentHealth> {
        let state = self.state.lock().unwrap();
        state
            .agent_state
            .get(&identifier.into())
            .and_then(|s| s.health_status.clone())
    }

    pub fn get_attributes(&self, identifier: impl Into<InstanceUid>) -> Option<AgentDescription> {
        let state = self.state.lock().unwrap();
        state
            .agent_state
            .get(&identifier.into())
            .map(|s| s.attributes.clone())
    }

    pub fn get_effective_config(
        &self,
        identifier: impl Into<InstanceUid>,
    ) -> Option<EffectiveConfig> {
        let state = self.state.lock().unwrap();
        state
            .agent_state
            .get(&identifier.into())
            .map(|s| s.effective_config.clone())
    }

    pub fn get_remote_config_status(
        &self,
        identifier: impl Into<InstanceUid>,
    ) -> Option<RemoteConfigStatus> {
        let state = self.state.lock().unwrap();
        state
            .agent_state
            .get(&identifier.into())
            .map(|s| s.config_status.clone())
    }

    /// Returns the instance IDs of all connected agents that have an identifying attribute
    /// matching the given key–value pair. Matches string-typed attribute values only.
    pub fn find_agents_with_identifying_attr(&self, key: &str, value: &str) -> Vec<InstanceID> {
        let state = self.state.lock().unwrap();
        state
            .agent_state
            .iter()
            .filter(|(_, agent_state)| has_identifying_attr(&agent_state.attributes, key, value))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Finds the Agent Control instance ID connected to this OpAMP server.
    /// Returns an error if no agent-control is connected or if more than one is found.
    pub fn find_agent_control_instance(&self) -> Result<InstanceID, String> {
        let agents = self.find_agents_with_identifying_attr("supervisor.key", "agent-control");
        match agents.len() {
            0 => Err("no agent-control connected to OpAMP server yet".to_string()),
            1 => Ok(agents.into_iter().next().unwrap()),
            n => Err(format!("expected exactly one agent-control, found {n}")),
        }
    }

    /// Returns the string value of an identifying attribute for the given agent, or `None` if the
    /// agent is unknown or the attribute is absent or not a string.
    pub fn get_identifying_attr_value(
        &self,
        identifier: impl Into<InstanceUid>,
        key: &str,
    ) -> Option<String> {
        let state = self.state.lock().unwrap();
        state
            .agent_state
            .get(&identifier.into())
            .and_then(|s| find_string_identifying_attr(&s.attributes, key))
    }

    /// Returns `Ok(())` if the agent has reported a remote config status of `Applied`.
    pub fn is_config_status_applied(
        &self,
        identifier: impl Into<InstanceUid>,
    ) -> Result<(), String> {
        match self.get_remote_config_status(identifier) {
            Some(s) if s.status == RemoteConfigStatuses::Applied as i32 => Ok(()),
            Some(_) => Err("Config status is not Applied".to_string()),
            None => Err("Remote config status not found".to_string()),
        }
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn opamp_handler(state: web::Data<Arc<Mutex<ServerState>>>, req: web::Bytes) -> HttpResponse {
    let message = AgentToServer::decode(req).unwrap();
    let identifier = InstanceUid::try_from(message.clone().instance_uid).unwrap();

    let mut server_state = state.lock().unwrap();

    let agent_state = server_state
        .agent_state
        .entry(identifier.clone())
        .or_default();

    let mut flags = ServerToAgentFlags::Unspecified as u64;
    if message.sequence_num == (agent_state.sequence_number + 1) {
        agent_state.sequence_number += 1;
    } else {
        flags = ServerToAgentFlags::ReportFullState as u64;
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

    if let Some(cfg_status) = message.remote_config_status {
        if agent_state
            .remote_config
            .as_ref()
            .is_some_and(|config_response| {
                config_response.0.config_hash == cfg_status.last_remote_config_hash
            })
        {
            agent_state.remote_config = None;
        }
        agent_state.config_status = cfg_status;
    }

    let remote_config = agent_state.remote_config.clone();

    let _ = agent_state; // drop mutable ref before taking immutable one

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
                "use": "sig",
                "kid": JWKS_PUBLIC_KEY_ID,
                "x": enc_public_key,
                "crv": ED25519_ALG,
            }
        ]
    });
    HttpResponse::Ok().json(payload)
}

fn build_response(
    instance_id: InstanceUid,
    maybe_remote_config: Option<RemoteConfig>,
    key_pair: &Ed25519KeyPair,
    flags: u64,
) -> Vec<u8> {
    let mut maybe_agent_remote_config = None;
    let mut maybe_custom_message = None;

    if let Some(remote_config) = maybe_remote_config {
        maybe_custom_message = Some(RemoteConfigSignature::new(key_pair, remote_config.clone()).0);
        maybe_agent_remote_config = Some(remote_config.0);
    }

    ServerToAgent {
        instance_uid: Vec::<u8>::from(instance_id),
        remote_config: maybe_agent_remote_config,
        custom_message: maybe_custom_message,
        flags,
        ..Default::default()
    }
    .encode_to_vec()
}

fn find_string_identifying_attr(description: &AgentDescription, key: &str) -> Option<String> {
    let kv = description
        .identifying_attributes
        .iter()
        .find(|kv| kv.key == key)?;

    match kv.value.as_ref()?.value.as_ref()? {
        Value::StringValue(s) => Some(s.clone()),
        _ => None,
    }
}

fn has_identifying_attr(description: &AgentDescription, key: &str, value: &str) -> bool {
    find_string_identifying_attr(description, key).is_some_and(|v| v == value)
}

fn generate_key_pair() -> Ed25519KeyPair {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
}
