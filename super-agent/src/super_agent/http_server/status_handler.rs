use std::sync::Arc;

use actix_web::http::header::ContentType;
use actix_web::web::Data;
use actix_web::{HttpResponse, Responder};
use tokio::sync::RwLock;

use crate::super_agent::http_server::status::Status;

pub(super) async fn status_handler(status: Data<Arc<RwLock<Status>>>) -> impl Responder {
    let status = status.read().await;
    let body = serde_json::to_string(&*status).unwrap();

    // Create response and set content type
    HttpResponse::Ok()
        .content_type(ContentType::json())
        .body(body)
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::opamp::Endpoint;
    use actix_web::body::MessageBody;
    use actix_web::test::TestRequest;
    use actix_web::web::Data;
    use actix_web::Responder;
    use tokio::sync::RwLock;

    use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
    use crate::super_agent::config::{AgentID, AgentTypeFQN};
    use crate::super_agent::http_server::status::{Status, SubAgentStatus};
    use crate::super_agent::http_server::status_handler::status_handler;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_handler_without_optional_fields() {
        // Given there is a healthy Sub Agent registered
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentTypeFQN::from("some-agent-type");
        let mut sub_agent_status =
            SubAgentStatus::with_id_and_type(agent_id.clone(), agent_type.clone());
        sub_agent_status.healthy(Healthy::default());

        let sub_agents = HashMap::from([(agent_id.clone(), sub_agent_status)]);

        let mut st = Status::default()
            .with_sub_agents(sub_agents.into())
            .with_opamp(Endpoint::from("some_endpoint"));

        st.super_agent.healthy(Healthy::default());
        st.opamp.reachable();

        let status = Arc::new(RwLock::new(st));

        let data = Data::new(status);
        let responder = status_handler(data).await;

        let request = TestRequest::default().to_http_request();
        let response = responder.respond_to(&request);

        let expected_body = r#"{"super_agent":{"healthy":true},"opamp":{"enabled":true,"endpoint":"some_endpoint","reachable":true},"sub_agents":{"some-agent-id":{"agent_id":"some-agent-id","agent_type":"some-agent-type","healthy":true}}}"#;

        assert_eq!(
            expected_body,
            response
                .map_into_boxed_body()
                .into_body()
                .try_into_bytes()
                .unwrap()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_handler() {
        // Given there is a healthy Sub Agent registered
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentTypeFQN::from("some-agent-type");
        let mut sub_agent_status =
            SubAgentStatus::with_id_and_type(agent_id.clone(), agent_type.clone());
        sub_agent_status.unhealthy(Unhealthy {
            last_error: String::from("a sub agent error"),
            ..Default::default()
        });

        let sub_agents = HashMap::from([(agent_id.clone(), sub_agent_status)]);

        let mut st = Status::default()
            .with_sub_agents(sub_agents.into())
            .with_opamp(Endpoint::from("some_endpoint"));

        st.super_agent.unhealthy(Unhealthy {
            last_error: String::from("this is an error"),
            ..Default::default()
        });
        st.opamp.reachable();

        let status = Arc::new(RwLock::new(st));

        let data = Data::new(status);
        let responder = status_handler(data).await;

        let request = TestRequest::default().to_http_request();
        let response = responder.respond_to(&request);

        let expected_body = r#"{"super_agent":{"healthy":false,"last_error":"this is an error"},"opamp":{"enabled":true,"endpoint":"some_endpoint","reachable":true},"sub_agents":{"some-agent-id":{"agent_id":"some-agent-id","agent_type":"some-agent-type","healthy":false,"last_error":"a sub agent error"}}}"#;

        assert_eq!(
            expected_body,
            response
                .map_into_boxed_body()
                .into_body()
                .try_into_bytes()
                .unwrap()
        );
    }
}
