//! Tools to set up a [tracing_subscriber] to report instrumentation.

use super::{
    config::{
        logs::config::{LoggingConfig, LoggingConfigError},
        InstrumentationConfig,
    },
    exporters::{
        file::file,
        otel::{config::OtelConfig, exporter::OtelExporter},
        stdout::stdout,
    },
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

/// This trait represent any exporter whose resources need to be live while the application
/// reports instrumentation.
pub trait InstrumentationExporter {}

/// Type to represent any [InstrumentationExporter] whose type will be known at runtime.
pub type InstrumentationExporterBox = Box<dyn InstrumentationExporter>;

/// Allows using a collection of instrumentation exporters as instrumentation exporter.
impl InstrumentationExporter for Vec<InstrumentationExporterBox> {}

/// Represents a registry layer to report tracing data to any destination.
/// Check [tracing_subscriber::Layer] and [tracing_subscriber::Registry] for details.
pub type LayerBox = Box<dyn Layer<Registry> + Send + Sync + 'static>;

// TODO: move this to the corresponding module
impl InstrumentationExporter for OtelExporter {}

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
/// # use newrelic_agent_control::instrumentation::config::{InstrumentationConfig, logs::config::LoggingConfig};
/// # use std::path::PathBuf;
///
/// let tracing_config = TracingConfig::new(
///     PathBuf::from("/some/path"),
///     LoggingConfig::default(),
///     InstrumentationConfig::default(),
/// );
/// let tracer = try_init_tracing(tracing_config).expect("could not initialize tracing");
///
/// tracing::info!("some instrumentation");
/// ```
pub fn try_init_tracing(config: TracingConfig) -> Result<InstrumentationExporterBox, TracingError> {
    // Currently stdout output is always on, we could consider allowing to turn it off.
    let mut layers = Vec::from([stdout(&config.logging_config)?]);
    let mut exporters = Vec::<InstrumentationExporterBox>::new();

    if let Some((file_layer, file_guard)) = file(&config.logging_config, config.logging_path)? {
        layers.push(file_layer);
        exporters.push(Box::new(file_guard));
    }

    if let Some(otel_config) = config.instrumentation_config.opentelemetry.as_ref() {
        let otel_exporter = build_otel_exporter(otel_config)?;
        // TODO: otel will eventually be one layer only
        let mut otel_layers = otel_exporter.tracing_layers();
        layers.append(&mut otel_layers);
        // TODO: explain why this is needed
        otel_exporter.set_global();
        // TODO: check if consuming the exporter when building the layers would be enough to properly shutdown.
        exporters.push(Box::new(otel_exporter));
    }
    try_init_tracing_subscriber(layers)?;
    debug!("tracing_subscriber initialized successfully");

    Ok(Box::new(exporters))
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

// TODO: check naming and if this belong to the otel module
fn build_otel_exporter(otel_config: &OtelConfig) -> Result<OtelExporter, TracingError> {
    let http_config = HttpConfig::new(
        otel_config.client_timeout.clone().into(),
        otel_config.client_timeout.clone().into(),
        otel_config.proxy.clone(),
    );
    let http_client = HttpClient::new(http_config).map_err(|err| {
        TracingError::Otel(format!("could not build the otel http client: {err}"))
    })?;
    let otel_providers = OtelExporter::try_new(otel_config, http_client).map_err(|err| {
        TracingError::Otel(format!(
            "could not build the OpenTelemetry providers: {err}"
        ))
    })?;
    Ok(otel_providers)
}
