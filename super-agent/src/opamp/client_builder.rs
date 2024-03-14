use crate::auth::token::TokenRetriever;
use crate::event::channel::EventPublisher;
use crate::event::OpAMPEvent;
use crate::opamp::auth_http_client::AuthHttpClient;
use crate::opamp::instance_id;
use crate::super_agent::config::{AgentID, OpAMPClientConfig};
use opamp_client::http::config::HttpConfigError;
use opamp_client::http::http_client::HttpClient;
use opamp_client::http::{HttpClientError, HttpClientUreq, HttpConfig};
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::operation::settings::StartSettings;
use opamp_client::{NotStartedClientError, StartedClient, StartedClientError};
use std::sync::Arc;
use std::time::SystemTimeError;
use thiserror::Error;
use tracing::error;

#[derive(Error, Debug)]
pub enum OpAMPClientBuilderError {
    #[error("unable to create OpAMP HTTP client: `{0}`")]
    HttpClientError(#[from] HttpClientError),
    #[error("invalid HTTP configuration: `{0}`")]
    HttpConfigError(#[from] HttpConfigError),
    #[error("`{0}`")]
    NotStartedClientError(#[from] NotStartedClientError),
    #[error("`{0}`")]
    StartedClientError(#[from] StartedClientError),
    #[error("`{0}`")]
    StartedOpAMPlientError(#[from] opamp_client::ClientError),
    #[error("system time error: `{0}`")]
    SystemTimeError(#[from] SystemTimeError),
    #[error("error getting agent ulid: `{0}`")]
    GetUlidError(#[from] instance_id::GetterError),
}

pub trait OpAMPClientBuilder<CB: Callbacks> {
    type Client: StartedClient<CB> + 'static;
    // type StartedClient: StartedClient;
    fn build_and_start(
        &self,
        opamp_publisher: EventPublisher<OpAMPEvent>,
        agent_id: AgentID,
        start_settings: StartSettings,
    ) -> Result<Self::Client, OpAMPClientBuilderError>;
}

pub fn build_http_client<T: TokenRetriever>(
    config: &OpAMPClientConfig,
    token_retriever: Arc<T>,
) -> Result<AuthHttpClient<T>, OpAMPClientBuilderError> {
    let headers = config.headers.clone().unwrap_or_default();
    let headers: Vec<(&str, &str)> = headers
        .iter()
        .map(|(h, v)| (h.as_str(), v.as_str()))
        .collect();

    let ureq_http_client =
        HttpClientUreq::new(HttpConfig::new(config.endpoint.as_str())?.with_headers(headers)?)?;

    let auth_http_client = AuthHttpClient::new(ureq_http_client, token_retriever);

    Ok(auth_http_client)
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use mockall::{mock, predicate};
    use opamp_client::operation::settings::StartSettings;
    use opamp_client::ClientError;
    use opamp_client::{
        opamp::proto::{AgentDescription, AgentHealth, RemoteConfigStatus},
        Client, ClientResult, NotStartedClient, NotStartedClientResult, StartedClient,
        StartedClientResult,
    };

    mock! {
        pub NotStartedOpAMPClientMock {}
        impl NotStartedClient for NotStartedOpAMPClientMock
         {
            type StartedClient<C: Callbacks + Send + Sync + 'static> = MockStartedOpAMPClientMock<C>;
            fn start<C: Callbacks + Send + Sync + 'static>(self, callbacks: C, start_settings: StartSettings ) -> NotStartedClientResult<<Self as NotStartedClient>::StartedClient<C>>;
        }
    }

    mock! {
        pub StartedOpAMPClientMock<C> where C: Callbacks {}

        impl<C> StartedClient<C> for StartedOpAMPClientMock<C>
            where
            C: Callbacks + Send + Sync + 'static {

            fn stop(self) -> StartedClientResult<()>;
        }

        impl<C> Client for StartedOpAMPClientMock<C>
        where
        C: Callbacks + Send + Sync + 'static {

             fn set_agent_description(
                &self,
                description: AgentDescription,
            ) -> ClientResult<()>;

             fn set_health(&self, health: AgentHealth) -> ClientResult<()>;

             fn update_effective_config(&self) -> ClientResult<()>;

             fn set_remote_config_status(&self, status: RemoteConfigStatus) -> ClientResult<()>;
        }
    }

    impl<C> MockStartedOpAMPClientMock<C>
    where
        C: Callbacks + Send + Sync + 'static,
    {
        pub fn should_set_health(&mut self, times: usize) {
            self.expect_set_health().times(times).returning(|_| Ok(()));
        }

        pub fn should_set_specific_health(&mut self, times: usize, health: AgentHealth) {
            self.expect_set_health()
                .with(predicate::eq(health))
                .times(times)
                .returning(|_| Ok(()));
        }

        #[allow(dead_code)]
        pub fn should_not_set_health(&mut self, times: usize, status_code: u16, error_msg: String) {
            self.expect_set_health().times(times).returning(move |_| {
                Err(ClientError::SenderError(
                    HttpClientError::UnsuccessfulResponse(status_code, error_msg.clone()),
                ))
            });
        }
        pub fn should_stop(&mut self, times: usize) {
            self.expect_stop().times(times).returning(|| Ok(()));
        }

        #[allow(dead_code)]
        pub fn should_not_stop(&mut self, times: usize, status_code: u16, error_msg: String) {
            self.expect_stop().times(times).returning(move || {
                Err(StartedClientError::SyncClientError(
                    ClientError::SenderError(HttpClientError::UnsuccessfulResponse(
                        status_code,
                        error_msg.clone(),
                    )),
                ))
            });
        }

        // assertion just for the call of the method but not the remote
        // status itself (so any remote config status)
        pub fn should_set_any_remote_config_status(&mut self, times: usize) {
            self.expect_set_remote_config_status()
                .times(times)
                .returning(|_| Ok(()));
        }

        // assertion just for the call of the method but not the remote
        // status itself (so any remote config status)
        pub fn should_set_remote_config_status(&mut self, status: RemoteConfigStatus) {
            self.expect_set_remote_config_status()
                .once()
                .with(predicate::eq(status))
                .returning(|_| Ok(()));
        }
    }

    mock! {
        pub OpAMPClientBuilderMock<C> where C: Callbacks + Send + Sync + 'static{}

        impl<C> OpAMPClientBuilder<C> for OpAMPClientBuilderMock<C> where C: Callbacks + Send + Sync + 'static{
            type Client = MockStartedOpAMPClientMock<C>;
            fn build_and_start(&self, opamp_publisher: EventPublisher<OpAMPEvent>, agent_id: AgentID, start_settings: StartSettings) -> Result<<Self as OpAMPClientBuilder<C>>::Client, OpAMPClientBuilderError>;
        }
    }

    impl<C> MockOpAMPClientBuilderMock<C>
    where
        C: Callbacks + Send + Sync + 'static,
    {
        pub fn should_build_and_start(
            &mut self,
            agent_id: AgentID,
            start_settings: StartSettings,
            client: MockStartedOpAMPClientMock<C>,
        ) {
            self.expect_build_and_start()
                .with(
                    predicate::always(),
                    predicate::eq(agent_id),
                    predicate::eq(start_settings),
                )
                .once()
                .return_once(move |_, _, _| Ok(client));
        }
    }
}
