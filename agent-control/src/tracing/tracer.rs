use std::fmt::Debug;
use thiserror::Error;
use tracing::debug;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{Layer, Registry};

/// An enum representing possible errors during the logging initialization.
#[derive(Error, Debug)]
pub enum TracerError {
    #[error("init logging error: `{0}`")]
    TryInitError(String),
}

pub struct Tracer;
impl Tracer {
    /// Attempts to initialize the tracing subscriber with the inner configuration.
    pub fn try_init(layers: Box<dyn Layer<Registry> + Send + Sync>) -> Result<(), TracerError> {
        // a `Layer` wrapped in an `Option` such as the above defined `file_layer` also implements
        // the `Layer` trait. This allows individual layers to be enabled or disabled at runtime
        // while always producing a `Subscriber` of the same type.
        let tracer = tracing_subscriber::registry().with(layers);

        #[cfg(feature = "tokio-console")]
        let tracer = tracer.with(console_subscriber::spawn());

        tracer.try_init().map_err(|_| {
            TracerError::TryInitError("unable to set agent global tracer".to_string())
        })?;

        debug!("Tracer initialized successfully");
        Ok(())
    }
}
