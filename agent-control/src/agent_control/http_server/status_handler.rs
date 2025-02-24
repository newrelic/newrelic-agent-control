use crate::agent_control::http_server::status::Status;
use actix_web::http::header::ContentType;
use actix_web::web::Data;
use actix_web::{HttpResponse, Responder};
use std::sync::Arc;
use tokio::sync::RwLock;

pub(super) async fn status_handler(status: Data<Arc<RwLock<Status>>>) -> impl Responder {
    let status = status.read().await;
    let body = serde_json::to_string(&*status).unwrap();

    // Create response and set content type
    HttpResponse::Ok()
        .content_type(ContentType::json())
        .body(body)
}

#[cfg(test)]
mod tests {
    use crate::agent_control::config::{AgentID, AgentTypeFQN};
    use crate::agent_control::http_server::status::{Status, SubAgentStatus};
    use crate::agent_control::http_server::status_handler::status_handler;
    use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
    use crate::sub_agent::health::with_start_time::HealthWithStartTime;
    use crate::sub_agent::identity::AgentIdentity;
    use actix_web::body::MessageBody;
    use actix_web::test::TestRequest;
    use actix_web::web::Data;
    use actix_web::Responder;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::SystemTime;
    use tokio::sync::RwLock;
    use url::Url;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_handler_without_optional_fields() {
        // Given there is a healthy Sub Agent registered
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));
        let mut sub_agent_status = SubAgentStatus::with_identity(agent_identity.clone());

        let start_time = SystemTime::UNIX_EPOCH;

        sub_agent_status.update_health(HealthWithStartTime::new(
            Healthy::default().into(),
            start_time,
        ));

        let sub_agents = HashMap::from([(agent_identity.id, sub_agent_status)]);

        let mut st = Status::default()
            .with_sub_agents(sub_agents.into())
            .with_opamp(Url::try_from("http://127.0.0.1").unwrap());

        st.agent_control.healthy(Healthy::default());
        st.fleet.reachable();

        let status = Arc::new(RwLock::new(st));

        let data = Data::new(status);
        let responder = status_handler(data).await;

        let request = TestRequest::default().to_http_request();
        let response = responder.respond_to(&request);

        let expected_body = r#"{"agent_control":{"healthy":true},"fleet":{"enabled":true,"endpoint":"http://127.0.0.1/","reachable":true},"sub_agents":{"some-agent-id":{"agent_id":"some-agent-id","agent_type":"namespace/some-agent-type:0.0.1","healthy":true,"start_time_unix_nano":0,"status_time_unix_nano":0}}}"#;

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
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));
        let mut sub_agent_status = SubAgentStatus::with_identity(agent_identity.clone());
        sub_agent_status.update_health(HealthWithStartTime::new(
            Unhealthy::default()
                .with_last_error("some error".to_string())
                .into(),
            SystemTime::UNIX_EPOCH,
        ));

        let sub_agents = HashMap::from([(agent_identity.id, sub_agent_status)]);

        let mut st = Status::default()
            .with_sub_agents(sub_agents.into())
            .with_opamp(Url::try_from("http://127.0.0.1").unwrap());

        st.agent_control
            .unhealthy(Unhealthy::default().with_last_error("agent control error".to_string()));
        st.fleet.reachable();

        let status = Arc::new(RwLock::new(st));

        let data = Data::new(status);
        let responder = status_handler(data).await;

        let request = TestRequest::default().to_http_request();
        let response = responder.respond_to(&request);

        let expected_body = r#"{"agent_control":{"healthy":false,"last_error":"agent control error"},"fleet":{"enabled":true,"endpoint":"http://127.0.0.1/","reachable":true},"sub_agents":{"some-agent-id":{"agent_id":"some-agent-id","agent_type":"namespace/some-agent-type:0.0.1","healthy":false,"last_error":"some error","start_time_unix_nano":0,"status_time_unix_nano":0}}}"#;

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
