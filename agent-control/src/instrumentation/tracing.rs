//! Tools to set up a [tracing_subscriber] to report instrumentation.

use super::{
    config::InstrumentationConfig,
    logs::{
        self,
        config::{LoggingConfig, LoggingConfigError},
        layers::FileGuard,
    },
    otel::providers::OtelProviders,
};
use crate::http::{client::HttpClient, config::HttpConfig};
use std::path::PathBuf;
use thiserror::Error;
use tracing::debug;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer, Registry};

/// Represents errors while setting up or shutting down tracing.
#[derive(Error, Debug)]
pub enum TracingError {
    #[error("logging config error: {0}")]
    Logs(#[from] LoggingConfigError),
    #[error("could not start tracing: {0}")]
    Init(String),
    #[error("OpenTelemetry initialization error: {0}")]
    Otel(String),
}

/// Defines the behavior required to initialize a tracer.
pub trait Tracer {
    fn try_init(&self, layers: Vec<LayerBox>) -> Result<(), TracingError>;
}

/// Represents a registry layer to report tracing data to any destination.
/// Check [tracing_subscriber::Layer] and [tracing_subscriber::Registry] for details.
pub type LayerBox = Box<dyn Layer<Registry> + Send + Sync + 'static>;

/// Type to represent any [Tracer] whose type will be known at runtime.
pub type TracerBox = Box<dyn Tracer>;

/// Holds the information required to set up tracing.
pub struct TracingConfig {
    logging_path: PathBuf,
    logging_config: LoggingConfig,
    instrumentation_config: InstrumentationConfig,
}

impl TracingConfig {
    /// Returns a new instance.
    pub fn new(
        logging_path: PathBuf,
        logging_config: LoggingConfig,
        instrumentation_config: InstrumentationConfig,
    ) -> Self {
        Self {
            logging_path,
            logging_config,
            instrumentation_config,
        }
    }
}

/// This function allows initializing tracing as setup in the provided configuration.
///
/// Depending on the configuration, the tracer will be shutdown on drop, therefore the corresponding
/// instrumentation may not work as expected after it is dropped.
///
/// # Example:
/// ```
/// # use newrelic_agent_control::instrumentation::tracing::TracingConfig;
/// # use newrelic_agent_control::instrumentation::tracing::try_init_tracing;
/// # use newrelic_agent_control::instrumentation::logs::config::LoggingConfig;
/// # use newrelic_agent_control::instrumentation::config::InstrumentationConfig;
/// # use std::path::PathBuf;
///
/// let tracing_config = TracingConfig::new(
///     PathBuf::from("/some/path"),
///     LoggingConfig::default(),
///     InstrumentationConfig::default(),
/// );
/// let tracer = try_init_tracing(tracing_config);
///
/// tracing::info!("some instrumentation");
/// ```
pub fn try_init_tracing(config: TracingConfig) -> Result<TracerBox, TracingError> {
    // Currently stdout output is always on, we could consider allowing to turn it off.
    let mut layers = Vec::from([logs::layers::stdout(&config.logging_config)?]);
    let mut tracer: Box<dyn Tracer> = Box::new(SubscriberTracer {});

    if let Some((file_layer, file_guard)) =
        logs::layers::file(&config.logging_config, config.logging_path)?
    {
        layers.push(file_layer);
        tracer = Box::new(FileTracer::new(tracer, file_guard));
    }

    if let Some(otel_config) = config.instrumentation_config.opentelemetry.as_ref() {
        let http_config = HttpConfig::new(
            otel_config.client_timeout.clone().into(),
            otel_config.client_timeout.clone().into(),
            otel_config.proxy.clone(),
        );
        let http_client = HttpClient::new(http_config).map_err(|err| {
            TracingError::Otel(format!("could not build the otel http client: {err}"))
        })?;
        let otel_providers = OtelProviders::try_new(otel_config, http_client).map_err(|err| {
            TracingError::Otel(format!(
                "could not build the OpenTelemetry providers: {err}"
            ))
        })?;

        let mut otel_layers = otel_providers.tracing_layers();
        layers.append(&mut otel_layers);

        tracer = Box::new(OtelTracer::new(tracer, otel_providers));
    }

    tracer.try_init(layers)?;
    debug!("Tracer initialized successfully");

    Ok(tracer)
}

/// Implements a [Tracer] that registers a set of layers globally through [tracing_subscriber].
///
/// As a result, the initialization provided by this tracer is in charge of setting up tracer to be used
/// globally.
struct SubscriberTracer {}

impl Tracer for SubscriberTracer {
    fn try_init(&self, layers: Vec<LayerBox>) -> Result<(), TracingError> {
        let subscriber = tracing_subscriber::registry().with(layers);

        #[cfg(feature = "tokio-console")]
        let subscriber = subscriber.with(console_subscriber::spawn());

        subscriber.try_init().map_err(|err| {
            TracingError::Init(format!("unable to set agent global tracer: {err}"))
        })?;

        Ok(())
    }
}

/// Extends a [Tracer] by holding a [FileGuard] which needs to be kept while reporting instrumentation to the
/// corresponding file.
struct FileTracer {
    inner_tracer: Box<dyn Tracer>,
    _file_guard: FileGuard,
}

impl FileTracer {
    fn new(tracer: TracerBox, file_guard: FileGuard) -> Self {
        Self {
            inner_tracer: tracer,
            _file_guard: file_guard,
        }
    }
}

impl Tracer for FileTracer {
    fn try_init(&self, layers: Vec<LayerBox>) -> Result<(), TracingError> {
        self.inner_tracer.try_init(layers)
    }
}

/// Extends a [Tracer] with [OtelProviders]. The OpenTelemetry providers will be registered globally on
/// initialization and shut down when the instance is dropped.
struct OtelTracer {
    inner_tracer: Box<dyn Tracer>,
    otel_providers: Option<OtelProviders>,
}

impl OtelTracer {
    fn new(tracer: TracerBox, otel_providers: OtelProviders) -> Self {
        Self {
            inner_tracer: tracer,
            otel_providers: Some(otel_providers),
        }
    }
}

impl Tracer for OtelTracer {
    fn try_init(&self, layers: Vec<LayerBox>) -> Result<(), TracingError> {
        if let Some(otel_providers) = self.otel_providers.as_ref() {
            otel_providers.set_global()
        }
        self.inner_tracer.try_init(layers)
    }
}

impl Drop for OtelTracer {
    fn drop(&mut self) {
        if let Some(otel_providers) = self.otel_providers.take() {
            let _ = otel_providers.shutdown().inspect_err(
                |err| tracing::error!(%err, "error shutting down the OpenTelemetry providers"),
            );
        }
    }
}
