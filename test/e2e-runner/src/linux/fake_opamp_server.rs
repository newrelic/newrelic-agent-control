use actix_web::{App, HttpResponse, HttpServer, web};
use aws_lc_rs::digest;
use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair as _};
use base64::Engine;
use base64::prelude::{BASE64_STANDARD, BASE64_URL_SAFE_NO_PAD};
use opamp_client::opamp::proto::{
    AgentConfigFile, AgentConfigMap, AgentRemoteConfig, AgentToServer, CustomMessage,
    ServerToAgent, ServerToAgentFlags,
};
use prost::Message;
use serde_json::json;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};
use std::net;
use tokio::task::JoinHandle;
use tracing::info;

// OpAMP protocol constants — mirrors values from newrelic_agent_control
const FAKE_SERVER_PATH: &str = "/opamp-fake-server";
const JWKS_SERVER_PATH: &str = "/jwks";
const JWKS_PUBLIC_KEY_ID: &str = "fakeKeyName/0";
const AGENT_CONFIG_PREFIX: &str = "agentConfig";
const SIGNATURE_CUSTOM_CAPABILITY: &str = "com.newrelic.security.configSignature";
const SIGNATURE_CUSTOM_MESSAGE_TYPE: &str = "newrelicRemoteConfigSignature";

/// The raw OpAMP instance UID bytes as sent in the protobuf `instance_uid` field.
pub type InstanceID = Vec<u8>;

// --- Internal state ---

struct ServerState {
    agent_state: HashMap<InstanceID, AgentState>,
    key_pair: Ed25519KeyPair,
}

#[derive(Default)]
struct AgentState {
    sequence_number: u64,
    remote_config: Option<RemoteConfig>,
}

impl ServerState {
    fn generate() -> Self {
        Self {
            agent_state: HashMap::new(),
            key_pair: generate_key_pair(),
        }
    }
}

// --- RemoteConfig builder ---

#[derive(Clone, Debug, Default)]
struct RemoteConfig(AgentRemoteConfig);

impl RemoteConfig {
    fn new(config_map: HashMap<String, String>) -> Self {
        let built_map: HashMap<String, AgentConfigFile> = config_map
            .into_iter()
            .map(|(key, body)| {
                (
                    key,
                    AgentConfigFile {
                        body: body.into_bytes(),
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

    fn compute_hash(map: &HashMap<String, AgentConfigFile>) -> Vec<u8> {
        let mut hasher = DefaultHasher::new();
        for file in map.values() {
            file.body.hash(&mut hasher);
        }
        hasher.finish().to_string().into_bytes()
    }
}

// --- Signature ---

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SignatureFields {
    signature: String,
    signing_algorithm: String,
    key_id: String,
}

fn build_signature(key_pair: &Ed25519KeyPair, remote_config: &RemoteConfig) -> CustomMessage {
    let config_map = remote_config
        .0
        .config
        .as_ref()
        .map(|c| c.config_map.clone())
        .unwrap_or_default();

    let mut data: HashMap<String, Vec<SignatureFields>> = HashMap::new();
    for (cfg_key, cfg_content) in config_map {
        let digest = digest::digest(&digest::SHA256, &cfg_content.body);
        let msg = BASE64_STANDARD.encode(digest);
        let signature = key_pair.sign(msg.as_bytes());
        data.insert(
            cfg_key,
            vec![SignatureFields {
                signature: BASE64_STANDARD.encode(signature),
                signing_algorithm: "ED25519".to_string(),
                key_id: JWKS_PUBLIC_KEY_ID.to_string(),
            }],
        );
    }

    CustomMessage {
        capability: SIGNATURE_CUSTOM_CAPABILITY.to_string(),
        r#type: SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
        data: serde_json::to_vec(&data).unwrap(),
    }
}

// --- Tokio runtime (shared singleton) ---

fn tokio_runtime() -> Arc<tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<Arc<tokio::runtime::Runtime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            Arc::new(
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(3)
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for FakeOpAMPServer"),
            )
        })
        .clone()
}

// --- Public API ---

pub struct FakeOpAMPServer {
    handle: JoinHandle<()>,
    state: Arc<Mutex<ServerState>>,
    port: u16,
}

impl FakeOpAMPServer {
    pub fn start_new() -> Self {
        let listener = net::TcpListener::bind("0.0.0.0:0").expect("failed to bind OpAMP server");
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(ServerState::generate()));

        let handle = tokio_runtime().spawn(Self::run(listener, state.clone()));

        info!(port, "FakeOpAMPServer started");
        Self { handle, state, port }
    }

    pub fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}{}", self.port, FAKE_SERVER_PATH)
    }

    pub fn jwks_endpoint(&self) -> String {
        format!("http://127.0.0.1:{}{}", self.port, JWKS_SERVER_PATH)
    }

    /// Sets the remote config YAML to deliver to the agent with the given instance ID.
    /// The server will keep sending it until the agent acknowledges it (or until overwritten).
    pub fn set_config_response(&mut self, instance_id: InstanceID, yaml: impl Into<String>) {
        let mut state = self.state.lock().unwrap();
        state
            .agent_state
            .entry(instance_id)
            .or_default()
            .remote_config = Some(RemoteConfig::new(HashMap::from([(
            AGENT_CONFIG_PREFIX.to_string(),
            yaml.into(),
        )])));
    }

    async fn run(listener: net::TcpListener, state: Arc<Mutex<ServerState>>) {
        HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(state.clone()))
                .service(web::resource(FAKE_SERVER_PATH).to(opamp_handler))
                .service(web::resource(JWKS_SERVER_PATH).to(jwks_handler))
        })
        .listen(listener)
        .expect("FakeOpAMPServer: failed to listen")
        .run()
        .await
        .expect("FakeOpAMPServer: server error")
    }
}

impl Drop for FakeOpAMPServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

// --- HTTP handlers ---

async fn opamp_handler(
    state: web::Data<Arc<Mutex<ServerState>>>,
    req: web::Bytes,
) -> HttpResponse {
    let message = match AgentToServer::decode(req) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("FakeOpAMPServer: failed to decode AgentToServer: {e}");
            return HttpResponse::BadRequest().finish();
        }
    };

    let identifier: InstanceID = message.instance_uid.clone();

    let mut server_state = state.lock().unwrap();
    let agent_state = server_state
        .agent_state
        .entry(identifier.clone())
        .or_default();

    // Sequence number tracking
    let mut flags = ServerToAgentFlags::Unspecified as u64;
    if message.sequence_num == (agent_state.sequence_number + 1) {
        agent_state.sequence_number += 1;
    } else {
        flags = ServerToAgentFlags::ReportFullState as u64;
        agent_state.sequence_number = message.sequence_num;
    }

    // Clear remote config once agent acknowledges it
    if let Some(cfg_status) = message.remote_config_status {
        if agent_state
            .remote_config
            .as_ref()
            .is_some_and(|rc| rc.0.config_hash == cfg_status.last_remote_config_hash)
        {
            agent_state.remote_config = None;
        }
    }

    let remote_config = agent_state.remote_config.clone();
    let _ = agent_state;

    let response = build_response(identifier, remote_config, &server_state.key_pair, flags);
    HttpResponse::Ok().body(response)
}

async fn jwks_handler(
    state: web::Data<Arc<Mutex<ServerState>>>,
    _req: web::Bytes,
) -> HttpResponse {
    let server_state = state.lock().unwrap();
    let public_key = server_state.key_pair.public_key().as_ref().to_vec();
    let enc_public_key = BASE64_URL_SAFE_NO_PAD.encode(&public_key);
    let payload = json!({
        "keys": [{
            "kty": "OKP",
            "alg": null,
            "use": "sig",
            "kid": JWKS_PUBLIC_KEY_ID,
            "n": null,
            "x": enc_public_key,
            "y": null,
            "crv": "Ed25519"
        }]
    });
    HttpResponse::Ok().json(payload)
}

fn build_response(
    instance_id: InstanceID,
    maybe_remote_config: Option<RemoteConfig>,
    key_pair: &Ed25519KeyPair,
    flags: u64,
) -> Vec<u8> {
    let (maybe_agent_remote_config, maybe_custom_message) =
        if let Some(rc) = maybe_remote_config {
            let sig = build_signature(key_pair, &rc);
            (Some(rc.0), Some(sig))
        } else {
            (None, None)
        };

    ServerToAgent {
        instance_uid: instance_id,
        remote_config: maybe_agent_remote_config,
        custom_message: maybe_custom_message,
        flags,
        ..Default::default()
    }
    .encode_to_vec()
}

fn generate_key_pair() -> Ed25519KeyPair {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
}

// --- Read AC instance ID from disk ---

/// Path where Agent Control stores its OpAMP instance UID when running as a system service.
const AC_INSTANCE_ID_PATH: &str =
    "/var/lib/newrelic-agent-control/fleet-data/agent-control/instance_id.yaml";

/// Reads the Agent Control instance ID from disk.
/// Returns `None` if the file doesn't exist yet (AC hasn't started or hasn't connected to OpAMP).
pub fn read_ac_instance_id() -> Option<InstanceID> {
    let content = std::fs::read_to_string(AC_INSTANCE_ID_PATH).ok()?;
    let yaml: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;
    let id_str = yaml["instance_id"].as_str()?;

    // Convert ULID string (e.g. "018FF38D01B37796B2C81C8069BC6ADF") to raw bytes
    let uid: opamp_client::operation::instance_uid::InstanceUid =
        id_str.to_string().try_into().ok()?;
    Some(Vec::<u8>::from(uid))
}
