use super::runtime::tokio_runtime;
use actix_web::{web, App, HttpResponse, HttpServer};
use newrelic_super_agent::opamp::instance_id::InstanceID;
use opamp_client::opamp;
use prost::Message;
use std::sync::Mutex;
use std::time::Duration;
use std::{collections::HashMap, net, sync::Arc};
use tokio::task::JoinHandle;
use uuid::{Bytes, Uuid};

const FAKE_SERVER_PATH: &str = "/opamp-fake-server";

pub type ConfigResponses = HashMap<InstanceID, ConfigResponse>;

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
    fn encode(&self) -> Vec<u8> {
        // remote config is only set if there is any content
        let remote_config = self
            .clone()
            .raw_body
            .map(|raw_body| opamp::proto::AgentRemoteConfig {
                config_hash: "hash".into(), // fake has for the shake of simplicity
                config: Some(opamp::proto::AgentConfigMap {
                    config_map: HashMap::from([(
                        "".to_string(),
                        opamp::proto::AgentConfigFile {
                            body: raw_body.clone().into_bytes(),
                            content_type: " text/yaml".to_string(),
                        },
                    )]),
                }),
            });
        opamp::proto::ServerToAgent {
            instance_uid: "test".into(), // fake uid for the shake of simplicity
            remote_config,
            ..Default::default()
        }
        .encode_to_vec()
    }
}

/// FakeServer represents a OpAMP mock server that can be used for testing purposed.
/// The underlying http server will be aborted when the object is dropped.
pub struct FakeServer {
    handle: JoinHandle<()>,
    responses: Arc<Mutex<ConfigResponses>>,
    port: u16,
    path: String,
}

impl FakeServer {
    /// Gets the endpoint to be used in the Super-Agent static configuration.
    pub fn endpoint(&self) -> String {
        format!("http://localhost:{}{}", self.port, self.path)
    }

    /// Starts and returns new FakeServer in a random port.
    pub fn start_new() -> Self {
        let state = Arc::new(Mutex::new(ConfigResponses::default()));
        // While binding to port 0, the kernel gives you a free ephemeral port.
        let listener = net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = tokio_runtime().spawn(Self::run_http_server(listener, state.clone()));

        Self {
            handle,
            responses: state,
            port,
            path: FAKE_SERVER_PATH.to_string(),
        }
    }

    async fn run_http_server(listener: net::TcpListener, state: Arc<Mutex<ConfigResponses>>) {
        HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(state.clone()))
                .service(web::resource(FAKE_SERVER_PATH).to(config_handler))
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
        let mut responses = self.responses.lock().unwrap();
        responses.insert(identifier, response);
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

async fn config_handler(
    state: web::Data<Arc<Mutex<ConfigResponses>>>,
    req: web::Bytes,
) -> HttpResponse {
    tokio::time::sleep(Duration::from_secs(1)).await;
    let message = opamp::proto::AgentToServer::decode(req).unwrap();
    let instance_id = InstanceID::try_from(message.clone().instance_uid).unwrap();

    let mut config_responses = state.lock().unwrap();

    let config_response = config_responses
        .get_mut(&instance_id)
        .map(|config_response| {
            if remote_config_is_applied(&message) {
                config_response.raw_body = None;
            }
            config_response.to_owned()
        })
        .unwrap_or_default();

    HttpResponse::Ok().body(config_response.encode())
}

/// Checks if the remote is applied according to the agent message
fn remote_config_is_applied(message: &opamp::proto::AgentToServer) -> bool {
    if let Some(remote_config_status) = message.clone().remote_config_status {
        return opamp::proto::RemoteConfigStatuses::try_from(remote_config_status.status).unwrap()
            == opamp::proto::RemoteConfigStatuses::Applied;
    }
    false
}
