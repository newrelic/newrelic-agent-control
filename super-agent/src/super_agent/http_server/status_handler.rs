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

    use actix_web::body::MessageBody;
    use actix_web::test::TestRequest;
    use actix_web::web::Data;
    use actix_web::Responder;
    use tokio::sync::RwLock;

    use crate::super_agent::config::{AgentID, AgentTypeFQN};
    use crate::super_agent::http_server::status::{Status, SubAgentStatus};
    use crate::super_agent::http_server::status_handler::status_handler;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_handler() {
        // Given there is a healthy Sub Agent registered
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentTypeFQN::from("some-agent-type");
        let mut sub_agent_status = SubAgentStatus::new(agent_id.clone(), agent_type.clone());
        sub_agent_status.healthy();

        let sub_agents = HashMap::from([(agent_id.clone(), sub_agent_status)]);

        let mut st = Status::default().with_sub_agents(sub_agents.into());
        st.super_agent.healthy = true;
        st.opamp.enabled = true;
        st.opamp.endpoint = String::from("some_endpoint");
        st.opamp.reachable = true;

        let status = Arc::new(RwLock::new(st));

        let data = Data::new(status);
        let responder = status_handler(data).await;

        let request = TestRequest::default().to_http_request();
        let response = responder.respond_to(&request);

        let expected_body = r#"{"super_agent":{"healthy":true,"last_error":"","status":""},"opamp":{"enabled":true,"endpoint":"some_endpoint","reachable":true},"sub_agents":{"some-agent-id":{"agent_id":"some-agent-id","agent_type":"some-agent-type","healthy":true,"last_error":"","status":""}}}"#;

        assert_eq!(
            String::from(expected_body).into_bytes(),
            response
                .map_into_boxed_body()
                .into_body()
                .try_into_bytes()
                .unwrap()
                .to_vec()
        );
    }
}
