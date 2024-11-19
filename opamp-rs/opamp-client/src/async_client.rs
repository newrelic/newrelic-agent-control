//! OpAMP client async trait.

use crate::opamp::proto::RemoteConfigStatus;
use crate::opamp::proto::{AgentDescription, ComponentHealth};
use crate::operation::{callbacks::Callbacks, settings::StartSettings};
use crate::{AsyncClientResult, AsyncStartedClientResult, NotStartedClientResult};
use async_trait::async_trait;

#[async_trait]
/// Client defines the communication methods with the Opamp server.
/// It must be shared among threads safely.
pub trait AsyncClient: Send + Sync {
    /// set_agent_description sets attributes of the Agent. The attributes will be included
    /// in the next status report sent to the Server.
    async fn set_agent_description(&self, description: AgentDescription) -> AsyncClientResult<()>;

    /// set_health sets the health status of the Agent. The ComponentHealth will be included
    async fn set_health(&self, health: ComponentHealth) -> AsyncClientResult<()>;

    /// update_effective_config fetches the current local effective config using
    /// get_effective_config callback and sends it to the Server.
    async fn update_effective_config(&self) -> AsyncClientResult<()>;

    /// set_remote_config_status sets the current RemoteConfigStatus.
    async fn set_remote_config_status(&self, status: RemoteConfigStatus) -> AsyncClientResult<()>;

    // /// set_package_statuses sets the current PackageStatuses.
    // async fn set_package_statuses(&mut self, statuses: PackageStatuses) -> Result<(), Self::Error>;
}

/// A trait defining the methods necessary for managing a client in the OpAMP library.
#[async_trait]
pub trait AsyncNotStartedClient {
    /// Associated type for this UnstartedClient which implements the StartedClient trait.
    type AsyncStartedClient<C: Callbacks + Send + Sync + 'static>: AsyncStartedClient<C>;

    /// start the client and begin attempts to connect to the Server. Once a connection
    /// is established the client will attempt to maintain it by reconnecting if
    /// the connection is lost. All failed connections attempts will be reported via
    /// OnConnectFailed callback.
    ///
    /// Start may immediately return an error if the settings are incorrect (e.g. the
    /// serverURL is not a valid URL).
    ///
    /// Start does not wait until the connection to the Server is established and will
    /// likely return before the connection attempts are even made.
    ///
    /// It is guaranteed that after the Start() call returns without error one of the
    /// following callbacks will be called eventually (unless Stop() is called earlier):
    ///  - OnConnectFailed
    ///  - OnError
    ///  - OnRemoteConfig
    ///
    ///  Starts should periodically poll status updates from the remote server and apply
    ///  the corresponding updates.
    async fn start<C: Callbacks + Send + Sync + 'static>(
        self,
        callbacks: C,
        start_settings: StartSettings,
    ) -> NotStartedClientResult<Self::AsyncStartedClient<C>>;
}

/// A trait defining the `stop()` method for stopping a client in the OpAMP library. Implements the `AsyncClient` trait.
#[async_trait]
pub trait AsyncStartedClient<C: Callbacks>: AsyncClient {
    /// After this call returns successfully it is guaranteed that no
    /// callbacks will be called. stop() will cancel context of any in-fly
    /// callbacks, but will wait until such in-fly callbacks are returned before
    /// Stop returns, so make sure the callbacks don't block infinitely and react
    /// promptly to context cancellations.
    /// Once stopped OpAMPClient cannot be started again.
    async fn stop(self) -> AsyncStartedClientResult<()>;
}
