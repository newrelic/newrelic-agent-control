//! Tools to set up a [tracing_subscriber] to report instrumentation.

use crate::{agent_control::agent_id::AgentID, reporter::UptimeReporter};

use super::{
    config::{
        logs::config::{LoggingConfig, LoggingConfigError},
        InstrumentationConfig,
    },
    tracing_layers::{
        file::file,
        otel::{OtelBuildError, OtelLayers},
        stdout::stdout,
    },
};
use std::path::PathBuf;
use thiserror::Error;
use tracing::debug;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer, Registry};

/// Represents errors while setting up or shutting down tracing.
#[derive(Error, Debug)]
pub enum TracingError {
    #[error("could not initialize logging component: {0}")]
    Logs(#[from] LoggingConfigError),
    #[error("could not start tracing: {0}")]
    Init(String),
    #[error("could not initialize OpenTelemetry component: {0}")]
    Otel(#[from] OtelBuildError),
}

/// This trait represent any instrumentation data source whose resources cannot be dropped while application
/// reports instrumentation.
pub trait TracingGuard {}

/// Type to represent any [TracingGuard] whose type will be known at runtime.
pub type TracingGuardBox = Box<dyn TracingGuard>;

/// Represents a registry layer to report tracing data to any destination.
/// Check [tracing_subscriber::Layer] and [tracing_subscriber::Registry] for details.
pub type LayerBox = Box<dyn Layer<Registry> + Send + Sync + 'static>;

/// Holds the information required to set up tracing.
pub struct TracingConfig {
    logging_path: PathBuf,
    logging_config: LoggingConfig,
    instrumentation_config: InstrumentationConfig,
}

impl TracingConfig {
    /// Returns tracing config the logging path set only.
    pub fn from_logging_path(logging_path: PathBuf) -> Self {
        Self {
            logging_path,
            logging_config: Default::default(),
            instrumentation_config: Default::default(),
        }
    }

    /// Sets logging config in a new configuration instance
    pub fn with_logging_config(self, logging_config: LoggingConfig) -> Self {
        Self {
            logging_config,
            ..self
        }
    }

    /// Sets instrumentation config in a new configuration instance
    pub fn with_instrumentation_config(
        self,
        instrumentation_config: InstrumentationConfig,
    ) -> Self {
        Self {
            instrumentation_config,
            ..self
        }
    }
}

/// This function allows initializing tracing as setup in the provided configuration.
///
/// Depending on the configuration, the tracer might be shutdown on drop, therefore the corresponding
/// instrumentation may not work as expected after it is dropped.
///
/// # Example:
/// ```
/// # use newrelic_agent_control::instrumentation::tracing::TracingConfig;
/// # use newrelic_agent_control::instrumentation::tracing::try_init_tracing;
/// # use newrelic_agent_control::instrumentation::config::{InstrumentationConfig, logs::config::LoggingConfig};
/// # use std::path::PathBuf;
///
/// let tracing_config = TracingConfig::from_logging_path(PathBuf::from("/some/path"));
/// let _tracing_guard = try_init_tracing(tracing_config).expect("could not initialize tracing");
///
/// tracing::info!("some instrumentation");
/// ```
pub fn try_init_tracing(config: TracingConfig) -> Result<Vec<TracingGuardBox>, TracingError> {
    // Currently stdout output is always on, we could consider allowing to turn it off.
    let mut layers = Vec::from([stdout(&config.logging_config)?]);
    let mut guards = Vec::<TracingGuardBox>::new();

    if let Some((file_layer, file_guard)) = file(&config.logging_config, config.logging_path)? {
        layers.push(file_layer);
        guards.push(Box::new(file_guard));
    }

    if let Some(otel_config) = config.instrumentation_config.opentelemetry.as_ref() {
        layers.push(OtelLayers::try_build(otel_config)?);

        // Allows including the log information on spans that contain them when send to otlp.
        opentelemetry::global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );

        guards.push(Box::new(UptimeReporter::start(
            &AgentID::new_agent_control_id(),
            otel_config.uptime_reporter_interval,
        )));
    }
    try_init_tracing_subscriber(layers)?;
    debug!("tracing_subscriber initialized successfully");

    Ok(guards)
}

/// Sets up the tracing_subscriber corresponding to the provided layers to be used globally.
fn try_init_tracing_subscriber(layers: Vec<LayerBox>) -> Result<(), TracingError> {
    let subscriber = tracing_subscriber::registry().with(layers);

    #[cfg(feature = "tokio-console")]
    let subscriber = subscriber.with(console_subscriber::spawn());

    subscriber
        .try_init()
        .map_err(|err| TracingError::Init(format!("unable to set agent global tracer: {err}")))?;

    Ok(())
}
