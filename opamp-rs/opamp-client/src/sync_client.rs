//! OpAMP client async trait.

use crate::opamp::proto::RemoteConfigStatus;
use crate::opamp::proto::{AgentDescription, ComponentHealth};
use crate::operation::{callbacks::Callbacks, settings::StartSettings};
use crate::{ClientResult, NotStartedClientResult, StartedClientResult};

/// Client defines the communication methods with the Opamp server.
/// It must be shared among threads safely.
pub trait Client: Send + Sync {
    /// set_agent_description sets attributes of the Agent. The attributes will be included
    /// in the next status report sent to the Server.
    fn set_agent_description(&self, description: AgentDescription) -> ClientResult<()>;

    /// set_health sets the health status of the Agent. The ComponentHealth will be included
    fn set_health(&self, health: ComponentHealth) -> ClientResult<()>;

    /// update_effective_config fetches the current local effective config using
    /// get_effective_config callback and sends it to the Server.
    /// The reason why there is a callback to fetch the EffectiveConfig from the Agent and it is not
    /// sent by the Agent like health, is to allow the compression mechanism without storing it.
    fn update_effective_config(&self) -> ClientResult<()>;

    /// set_remote_config_status sets the current RemoteConfigStatus.
    fn set_remote_config_status(&self, status: RemoteConfigStatus) -> ClientResult<()>;
}

/// A trait defining the methods necessary for managing a client in the OpAMP library.
pub trait NotStartedClient {
    /// Associated type for this UnstartedClient which implements the StartedClient trait.
    type StartedClient<C: Callbacks + Send + Sync + 'static>: StartedClient<C>;

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
    fn start<C: Callbacks + Send + Sync + 'static>(
        self,
        callbacks: C,
        start_settings: StartSettings,
    ) -> NotStartedClientResult<Self::StartedClient<C>>;
}

/// A trait defining the `stop()` method for stopping a client in the OpAMP library. Implements the `Client` trait.
pub trait StartedClient<C: Callbacks>: Client {
    /// After this call returns successfully it is guaranteed that no
    /// callbacks will be called. stop() will cancel context of any in-fly
    /// callbacks, but will wait until such in-fly callbacks are returned before
    /// Stop returns, so make sure the callbacks don't block infinitely and react
    /// promptly to context cancellations.
    /// Once stopped OpAMPClient cannot be started again.
    fn stop(self) -> StartedClientResult<()>;
}
