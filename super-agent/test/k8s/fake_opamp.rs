use std::{collections::HashMap, sync::Arc};

use axum::{
    body::Bytes, extract::State, http::StatusCode, response::IntoResponse, routing, Router,
};
use opamp_client::opamp;
use prost::Message;
use tokio::{sync::Mutex, task::JoinHandle};

use crate::common::{block_on, tokio_runtime};

const FAKE_SERVER_PATH: &str = "/fake-server";

pub type Identifier = String;
pub type Responses = HashMap<Identifier, Response>;

pub struct FakeServer {
    handle: JoinHandle<()>,
    responses: Arc<Mutex<Responses>>,
    port: u16,
    path: String,
}

#[derive(Clone, Debug)]
pub struct Response {
    pub status: StatusCode,
    pub raw_body: String,
}

impl FakeServer {
    /// Gets the endpoint to be used in the Super-Agent static configuration.
    pub fn endpoint(&self) -> String {
        format!("http://localhost:{}{}", self.port, self.path)
    }

    /// Starts and returns new FakeServer in a random port with the provided responses.
    pub fn start_new(responses: Responses) -> Self {
        let responses = Arc::new(Mutex::new(responses));
        let path = FAKE_SERVER_PATH.to_string();

        let router = Router::new().route(
            &path,
            routing::post(request_handler).with_state(responses.clone()),
        );

        // While binding to port 0, the kernel gives you an ephemeral port that is free.
        let listener = block_on(tokio::net::TcpListener::bind("0.0.0.0:0")).unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = tokio_runtime().spawn(async {
            axum::serve(listener, router).await.unwrap();
        });

        Self {
            responses,
            handle,
            port,
            path,
        }
    }

    /// Sets a response for the provided identifier. If a response already existed, it is overwritten.
    pub fn set_response(&mut self, identifier: Identifier, response: Response) {
        block_on(self.set_response_async(identifier, response))
    }

    async fn set_response_async(&mut self, identifier: Identifier, response: Response) {
        let mut responses = self.responses.lock().await;
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

async fn request_handler(
    State(exp): State<Arc<Mutex<Responses>>>,
    request: Bytes,
) -> impl IntoResponse {
    let message = opamp::proto::AgentToServer::decode(request).unwrap();

    let identifier = response_identifier(&message);

    let responses = exp.lock().await;
    let response = responses.get(identifier.as_str()).unwrap();

    build_axum_response(response)
}

/// Gets the corresponding response identifier for the provided a OpAMP message.
/// # Panics
/// When the expected identifier is not set in the message or it doesn't have the expected format.
fn response_identifier(message: &opamp::proto::AgentToServer) -> String {
    // TODO: We use `service.name` for now, but we need to identify each agent separately.
    // `message.instance_uid` contains the ulid but it cannot be easily obtained from expectations.
    let agent_description = message.agent_description.clone().unwrap();
    let service_name_value = agent_description
        .identifying_attributes
        .iter()
        .find(|key_value| key_value.key == "service.name")
        .unwrap() // KeyValue
        .value // AnyValue
        .clone()
        .unwrap()
        .value // Value
        .unwrap();
    let service_name = match service_name_value {
        opamp::proto::any_value::Value::StringValue(value) => value.clone(),
        _ => panic!("'service.name' should be a string"),
    };
    service_name.to_string()
}

fn build_axum_response(response: &Response) -> impl IntoResponse {
    let (status_code, server_to_agent) = (
        response.status,
        opamp::proto::ServerToAgent {
            instance_uid: "test".into(), // the ulid is not checked by the client
            remote_config: Some(opamp::proto::AgentRemoteConfig {
                config_hash: "hash".into(), // hash is never checked so can be fake
                config: Some(opamp::proto::AgentConfigMap {
                    config_map: HashMap::from([(
                        "".to_string(),
                        opamp::proto::AgentConfigFile {
                            body: response.raw_body.clone().into_bytes(),
                            content_type: " text/yaml".to_string(),
                        },
                    )]),
                }),
            }),
            flags: 0,
            capabilities: 0,
            agent_identification: None,
            command: None,
            connection_settings: None,
            error_response: None,
            packages_available: None,
        },
    );
    (status_code, server_to_agent.encode_to_vec())
}
