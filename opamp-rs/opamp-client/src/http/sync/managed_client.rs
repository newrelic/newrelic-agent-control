//! Implementation of the NotStartedClient and StartedClient traits for OpAMP

use std::{
    sync::Arc,
    thread::{spawn, JoinHandle},
    time::Duration,
};
use tracing::warn;

use crate::{
    opamp::proto::RemoteConfigStatus, ClientError, StartedClient, StartedClientError,
    StartedClientResult,
};
use crate::{
    operation::{callbacks::Callbacks, settings::StartSettings},
    Client, ClientResult, NotStartedClient, NotStartedClientResult,
};

use super::{
    client::OpAMPHttpClient,
    http_client::HttpClient,
    ticker::{CrossBeamTicker, Ticker},
};

// Default and minimum interval for OpAMP
static DEFAULT_POLLING_INTERVAL: Duration = Duration::from_secs(30);

/// NotStartedHttpClient implements the NotStartedClient trait for HTTP.
pub struct NotStartedHttpClient<L, T = CrossBeamTicker>
where
    L: HttpClient + Send + Sync,
    T: Ticker + Send + Sync,
{
    ticker: T,
    http_client: L,
}

/// An HttpClient that frequently polls for OpAMP remote updates in a background thread
/// using HTTP transport for connections.
pub struct StartedHttpClient<C, L, T = CrossBeamTicker>
where
    T: Ticker + Send + Sync,
    C: Callbacks + Send + Sync,
    L: HttpClient + Send + Sync,
{
    // handle for the spawned polling task
    handle: JoinHandle<ClientResult<()>>,

    // restart_ticker is Ticker used to restart and stop the start routine
    ticker: Arc<T>,

    // Http opamp_client: TODO -> Mutex? One message at a time?
    opamp_client: Arc<OpAMPHttpClient<C, L>>,
}

impl<L> NotStartedHttpClient<L, CrossBeamTicker>
where
    L: HttpClient + Send + Sync,
{
    /// Creates a new instance of NotStartedHttpClient with provided parameters.
    pub fn new(http_client: L) -> Self {
        NotStartedHttpClient {
            http_client,
            ticker: CrossBeamTicker::new(DEFAULT_POLLING_INTERVAL),
        }
    }

    /// Returns a new instance of the NotStartedHttpClient with the specified interval for polling
    /// if the interval is smaller than default, a warning message will be printed and default
    /// value will be used
    pub fn with_interval(self, interval: Duration) -> NotStartedHttpClient<L, CrossBeamTicker> {
        let interval = if interval.le(&DEFAULT_POLLING_INTERVAL) {
            warn!(
                interval = interval.as_secs(),
                default_inverval = DEFAULT_POLLING_INTERVAL.as_secs(),
                "polling interval smaller than minimum. Falling back to default interval."
            );
            DEFAULT_POLLING_INTERVAL
        } else {
            interval
        };

        NotStartedHttpClient {
            ticker: CrossBeamTicker::new(interval),
            ..self
        }
    }
}

impl<L, T> NotStartedClient for NotStartedHttpClient<L, T>
where
    L: HttpClient + Send + Sync + 'static,
    T: Ticker + Send + Sync + 'static,
{
    type StartedClient<C: Callbacks + Send + Sync + 'static> = StartedHttpClient<C, L, T>;

    // Starts the NotStartedHttpClient and transforms it into an StartedHttpClient.
    fn start<C: Callbacks + Send + Sync + 'static>(
        self,
        callbacks: C,
        start_settings: StartSettings,
    ) -> NotStartedClientResult<Self::StartedClient<C>> {
        // use poll method to send an initial message
        tracing::debug!("sending first AgentToServer message");
        let opamp_client = Arc::new(OpAMPHttpClient::new(
            callbacks,
            start_settings,
            self.http_client,
        )?);

        opamp_client.poll()?;

        let ticker = Arc::new(self.ticker);

        let handle: JoinHandle<Result<(), ClientError>> = spawn({
            let opamp_client = opamp_client.clone();
            let ticker = ticker.clone();
            move || {
                while ticker.next().is_ok() {
                    tracing::debug!("sending polling request for a ServerToAgent message");
                    let _ = opamp_client
                        .poll()
                        .map_err(|err| tracing::error!("error while polling message: {}", err));
                }
                tracing::debug!("exiting http polling thread");
                Ok(())
            }
        });

        Ok(StartedHttpClient {
            handle,
            opamp_client,
            ticker,
        })
    }
}

impl<C, L, T> StartedClient<C> for StartedHttpClient<C, L, T>
where
    C: Callbacks + Send + Sync,
    L: HttpClient + Send + Sync,
    T: Ticker + Send + Sync + 'static,
{
    // Stops the StartedHttpClient, terminates the running background thread, and cleans up resources.
    fn stop(self) -> StartedClientResult<()> {
        self.ticker.stop()?;
        // explicitly drop the sender to cancel the started thread
        Ok(self
            .handle
            .join()
            .map_err(|_| StartedClientError::JoinError)??)
    }
}

impl<C, L, T> Client for StartedHttpClient<C, L, T>
where
    C: Callbacks + Send + Sync,
    L: HttpClient + Send + Sync,
    T: Ticker + Send + Sync + 'static,
{
    fn set_agent_description(
        &self,
        description: crate::opamp::proto::AgentDescription,
    ) -> ClientResult<()> {
        self.ticker.reset()?;
        self.opamp_client.set_agent_description(description)
    }

    /// set_health sets the health status of the Agent.
    fn set_health(&self, health: crate::opamp::proto::ComponentHealth) -> ClientResult<()> {
        self.ticker.reset()?;
        self.opamp_client.set_health(health)
    }

    // update_effective_config fetches the current local effective config using
    // get_effective_config callback and sends it to the Server.
    fn update_effective_config(&self) -> ClientResult<()> {
        self.ticker.reset()?;
        self.opamp_client.update_effective_config()
    }

    // update_effective_config fetches the current local effective config using
    // get_effective_config callback and sends it to the Server.
    fn set_remote_config_status(&self, status: RemoteConfigStatus) -> ClientResult<()> {
        self.ticker.reset()?;
        self.opamp_client.set_remote_config_status(status)
    }
}

#[cfg(test)]
mod test {

    use core::panic;
    use std::ops::{Add, Sub};
    use std::time::SystemTime;

    use super::super::http_client::test::{
        response_from_server_to_agent, MockHttpClientMockall, ResponseParts,
    };
    use crate::capabilities;
    use crate::common::clientstate::SyncedStateError;
    use crate::http::sync::ticker::test::MockTickerMockAll;
    use crate::opamp::proto::any_value::Value;
    use crate::opamp::proto::{
        AgentCapabilities, AgentDescription, AnyValue, KeyValue, ServerToAgent,
    };
    use crate::operation::callbacks::test::MockCallbacksMockall;
    use crate::operation::capabilities::Capabilities;

    use super::super::*;

    use super::*;

    #[test]
    fn start_stop() {
        // should be called one time (1 init)
        let mut mock_client = MockHttpClientMockall::new();
        mock_client.expect_post().times(2).returning(|_| {
            Ok(response_from_server_to_agent(
                &ServerToAgent::default(),
                ResponseParts::default(),
            ))
        });

        let mut mocked_callbacks = MockCallbacksMockall::new();
        mocked_callbacks.should_on_connect();
        mocked_callbacks
            .expect_on_message()
            .times(1) // 1 init
            .return_const(());

        let not_started = NotStartedHttpClient::new(mock_client);

        let start_result = not_started.start(
            mocked_callbacks,
            StartSettings {
                instance_id: "NOT_AN_UID".into(),
                capabilities: Capabilities::default(),
                ..Default::default()
            },
        );

        assert!(
            start_result.is_ok(),
            "unable to start the http opamp_client without any error"
        );

        assert!(
            start_result.unwrap().stop().is_ok(),
            "unable to stop the http opamp_client without any error"
        );
    }

    #[test]
    fn poll_and_set_health() {
        // should be called 5 times (1 init + 4 polling + 4 set_health - 2 set_health skipped)
        let mut mock_client = MockHttpClientMockall::new();
        mock_client.expect_post().times(8).returning(|_| {
            Ok(response_from_server_to_agent(
                &ServerToAgent::default(),
                ResponseParts::default(),
            ))
        });

        // ticker that will be cancelled after three calls
        let mut ticker = MockTickerMockAll::new();
        ticker.expect_next().times(4).returning(|| Ok(()));
        ticker
            .expect_next()
            .returning(|| Err(TickerError::Cancelled));

        ticker.expect_reset().times(4).returning(|| Ok(())); // set_health
        ticker.expect_stop().times(1).returning(|| Ok(()));

        let mut mocked_callbacks = MockCallbacksMockall::new();
        mocked_callbacks
            .expect_on_connect()
            .times(1 + 4 + 2)
            .return_const(()); // 1 init, 3 polls, 2 set_health
        mocked_callbacks
            .expect_on_message()
            .times(1 + 4 + 2) // 1 init, 3 polls, 2 set_health
            .return_const(());

        let not_started = NotStartedHttpClient {
            ticker,
            http_client: mock_client,
        };

        let client = not_started
            .start(
                mocked_callbacks,
                StartSettings {
                    instance_id: "NOT_AN_UID".into(),
                    capabilities: capabilities!(
                        AgentCapabilities::AcceptsRestartCommand,
                        AgentCapabilities::ReportsHealth
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let status_time_unix_nano = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let mut health = crate::opamp::proto::ComponentHealth {
            status_time_unix_nano,
            ..Default::default()
        };

        assert!(client.set_health(health.clone()).is_ok());
        // Same status should skip post
        assert!(client.set_health(health.clone()).is_ok());

        health.healthy = true;

        assert!(client.set_health(health.clone()).is_ok());
        // Same status should skip post
        assert!(client.set_health(health.clone()).is_ok());

        assert!(client.stop().is_ok())
    }

    #[test]
    fn poll_and_update_effective_config() {
        // should be called three times (1 init + 1 polling + 1 update_effective_config)
        let mut mock_client = MockHttpClientMockall::new();
        mock_client.expect_post().times(4).returning(|_| {
            Ok(response_from_server_to_agent(
                &ServerToAgent::default(),
                ResponseParts::default(),
            ))
        });

        // ticker that will be cancelled after one call
        let mut ticker = MockTickerMockAll::new();
        ticker.expect_next().once().returning(|| Ok(()));
        ticker
            .expect_next()
            .returning(|| Err(TickerError::Cancelled));

        ticker.expect_reset().times(1).returning(|| Ok(())); // update_effective_config
        ticker.expect_stop().times(1).returning(|| Ok(()));

        let mut mocked_callbacks = MockCallbacksMockall::new();
        mocked_callbacks
            .expect_on_message()
            .times(3) // 1 init, 1 poll, 1 update_effective_config
            .return_const(());
        mocked_callbacks
            .expect_on_connect()
            .times(3) // 1 init, 1 poll, 1 update_effective_config
            .return_const(());

        mocked_callbacks.should_get_effective_config();

        let not_started = NotStartedHttpClient {
            ticker,
            http_client: mock_client,
        };

        let client = not_started
            .start(
                mocked_callbacks,
                StartSettings {
                    instance_id: "NOT_AN_UID".into(),
                    capabilities: capabilities!(
                        crate::opamp::proto::AgentCapabilities::ReportsEffectiveConfig
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let res = client.update_effective_config();
        assert!(res.is_ok());
        assert!(client.stop().is_ok())
    }

    #[test]
    fn poll_and_set_agent_description() {
        // should be called three times (1 init + 1 polling + 1 set_agent_description)
        let mut mock_client = MockHttpClientMockall::new();
        mock_client.expect_post().times(4).returning(|_| {
            Ok(response_from_server_to_agent(
                &ServerToAgent::default(),
                ResponseParts::default(),
            ))
        });

        // ticker that will be cancelled after one call
        let mut ticker = MockTickerMockAll::new();
        ticker.expect_next().once().returning(|| Ok(()));
        ticker
            .expect_next()
            .returning(|| Err(TickerError::Cancelled));

        ticker.expect_reset().times(2).returning(|| Ok(())); // set_agent_description
        ticker.expect_stop().times(1).returning(|| Ok(()));

        let mut mocked_callbacks = MockCallbacksMockall::new();
        mocked_callbacks
            .expect_on_connect()
            .times(3) // 1 init, 1 poll, 1 set_agent_description
            .return_const(());
        mocked_callbacks
            .expect_on_message()
            .times(3) // 1 init, 1 poll, 1 set_agent_description
            .return_const(());

        let not_started = NotStartedHttpClient {
            ticker,
            http_client: mock_client,
        };

        let client = not_started
            .start(
                mocked_callbacks,
                StartSettings {
                    instance_id: "NOT_AN_UID".into(),
                    capabilities: capabilities!(
                        crate::opamp::proto::AgentCapabilities::ReportsEffectiveConfig
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let random_value = KeyValue {
            key: "thing".to_string(),
            value: Some(AnyValue {
                value: Some(Value::StringValue("thing_value".to_string())),
            }),
        };
        let res = client.set_agent_description(AgentDescription {
            identifying_attributes: vec![random_value.clone()],
            non_identifying_attributes: vec![random_value.clone()],
        });
        assert!(res.is_ok());
        // Second call doesn't post.
        let res = client.set_agent_description(AgentDescription {
            identifying_attributes: vec![random_value.clone()],
            non_identifying_attributes: vec![random_value],
        });
        assert!(res.is_ok());
        assert!(client.stop().is_ok())
    }

    #[test]
    fn poll_and_set_agent_description_no_attrs() {
        // should be called two times (1 init + 1 poll)
        let mut mock_client = MockHttpClientMockall::new();
        mock_client.expect_post().times(3).returning(|_| {
            Ok(response_from_server_to_agent(
                &ServerToAgent::default(),
                ResponseParts::default(),
            ))
        });

        // ticker that will be cancelled after one call
        let mut ticker = MockTickerMockAll::new();
        ticker.expect_next().once().returning(|| Ok(()));
        ticker
            .expect_next()
            .returning(|| Err(TickerError::Cancelled));

        ticker.expect_reset().times(1).returning(|| Ok(())); // set_agent_description
        ticker.expect_stop().times(1).returning(|| Ok(()));

        let mut mocked_callbacks = MockCallbacksMockall::new();
        mocked_callbacks
            .expect_on_connect()
            .times(2) // 1 init, 1 poll
            .return_const(());
        mocked_callbacks
            .expect_on_message()
            .times(2) // 1 init, 1 poll
            .return_const(());

        mocked_callbacks.should_not_get_effective_config();

        let not_started = NotStartedHttpClient {
            ticker,
            http_client: mock_client,
        };

        let client = not_started
            .start(
                mocked_callbacks,
                StartSettings {
                    instance_id: "NOT_AN_UID".into(),
                    capabilities: capabilities!(
                        crate::opamp::proto::AgentCapabilities::ReportsEffectiveConfig
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let res = client.set_agent_description(AgentDescription::default());
        assert!(res.is_err());

        match res.unwrap_err() {
            ClientError::SyncedStateError(e) =>  assert!(matches!(e, SyncedStateError::AgentDescriptionNoAttributes)),
            err => panic!("Wrong error variant was returned. Expected `ConnectionError::SyncedStateError`, found {}", err),
        }
        assert!(client.stop().is_ok())
    }

    #[test]
    fn poll_and_set_remote_config_status_three_calls_two_same_status() {
        // should be called three times (1 init + 2 statuses, last status is repeated)
        let mut mock_client = MockHttpClientMockall::new();
        mock_client.expect_post().times(4).returning(|_| {
            Ok(response_from_server_to_agent(
                &ServerToAgent::default(),
                ResponseParts::default(),
            ))
        });

        // ticker that will be cancelled after one call
        let mut ticker = MockTickerMockAll::new();
        ticker
            .expect_next()
            .returning(|| Err(TickerError::Cancelled));

        ticker.expect_reset().times(3).returning(|| Ok(())); // set_agent_description

        let mut mocked_callbacks = MockCallbacksMockall::new();
        mocked_callbacks
            .expect_on_connect()
            .times(3) // 1 init, 1 poll
            .return_const(());
        mocked_callbacks
            .expect_on_message()
            .times(3) // 1 init, 1 poll
            .return_const(());

        mocked_callbacks.should_not_get_effective_config();

        let not_started = NotStartedHttpClient {
            ticker,
            http_client: mock_client,
        };

        let client = not_started
            .start(
                mocked_callbacks,
                StartSettings {
                    instance_id: "NOT_AN_UID".into(),
                    capabilities: capabilities!(
                        crate::opamp::proto::AgentCapabilities::ReportsRemoteConfig
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let remote_config_status = RemoteConfigStatus {
            last_remote_config_hash: vec![],
            status: 1,
            error_message: "".to_string(),
        };
        let res = client.set_remote_config_status(remote_config_status);

        assert!(res.is_ok());

        let remote_config_status = RemoteConfigStatus {
            last_remote_config_hash: vec![],
            status: 2,
            error_message: "".to_string(),
        };
        let res = client.set_remote_config_status(remote_config_status);
        assert!(res.is_ok());

        let remote_config_status = RemoteConfigStatus {
            last_remote_config_hash: vec![],
            status: 2,
            error_message: "".to_string(),
        };
        let res = client.set_remote_config_status(remote_config_status);
        assert!(res.is_ok());
    }

    #[test]
    fn test_polling_interval() {
        // Default interval
        let http_client = MockHttpClientMockall::new();
        let opamp_client = NotStartedHttpClient::new(http_client);
        assert_eq!(opamp_client.ticker.duration(), DEFAULT_POLLING_INTERVAL);

        // Bigger interval than default should be allowed
        let http_client = MockHttpClientMockall::new();
        let new_interval = DEFAULT_POLLING_INTERVAL.add(Duration::from_secs(1));
        let opamp_client = NotStartedHttpClient::new(http_client).with_interval(new_interval);
        assert_eq!(opamp_client.ticker.duration(), new_interval);

        // Smaller interval than default should not be allowed
        let http_client = MockHttpClientMockall::new();
        let new_interval = DEFAULT_POLLING_INTERVAL.sub(Duration::from_secs(1));
        let opamp_client = NotStartedHttpClient::new(http_client).with_interval(new_interval);
        assert_eq!(opamp_client.ticker.duration(), DEFAULT_POLLING_INTERVAL);
    }
}
