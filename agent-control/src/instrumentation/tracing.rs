//! Tools to set up a [tracing_subscriber] to report instrumentation.

use super::{
    config::{
        InstrumentationConfig,
        logs::config::{LoggingConfig, LoggingConfigError},
    },
    tracing_layers::{
        file::file,
        otel::{OtelBuildError, OtelLayers},
        stderr::stderr,
    },
};
use crate::environment::Environment;
use std::path::PathBuf;
use thiserror::Error;
use tracing::debug;
use tracing_subscriber::{Layer, Registry, layer::SubscriberExt, util::SubscriberInitExt};

/// Runtime facts about the current Agent Control instance that aren't user-configurable but are
/// needed so self-instrumentation telemetry can identify and be grouped by instance (deployment
/// platform, k8s cluster, fleet) the same way OpAMP's `agent_description` already can.
#[derive(Debug, Clone)]
pub struct InstanceContext {
    /// Deployment platform this AC binary is running as (`linux` / `windows` / `kubernetes`).
    pub environment: Environment,
    /// K8s cluster name, when running in k8s mode.
    pub cluster_name: Option<String>,
    /// Fleet identifier, when this instance is connected to Fleet Control.
    pub fleet_id: Option<String>,
    /// K8s pod name, when running in k8s mode (from the `POD_NAME` downward-API env var).
    /// Distinct from `host.name`: k8s sets a pod's own OS hostname equal to its pod name, so
    /// without this, self-instrumentation has no attribute for the underlying node.
    pub pod_name: Option<String>,
    /// K8s node name, when running in k8s mode (from the `NODE_NAME` downward-API env var).
    pub node_name: Option<String>,
}

/// Represents errors while setting up or shutting down tracing.
#[derive(Error, Debug)]
pub enum TracingError {
    /// The logging component could not be initialized.
    #[error("could not initialize logging component: {0}")]
    Logs(#[from] LoggingConfigError),
    /// Tracing could not be started (e.g. setting the global subscriber failed).
    #[error("could not start tracing: {0}")]
    Init(String),
    /// The OpenTelemetry component could not be initialized.
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
    instance_context: Option<InstanceContext>,
}

impl TracingConfig {
    /// Returns tracing config the logging path set only.
    pub fn from_logging_path(logging_path: PathBuf) -> Self {
        Self {
            logging_path,
            logging_config: Default::default(),
            instrumentation_config: Default::default(),
            instance_context: None,
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

    /// Sets the instance context (deployment platform, k8s cluster, fleet) used to enrich
    /// self-instrumentation telemetry with identifying attributes.
    pub fn with_instance_context(self, instance_context: InstanceContext) -> Self {
        Self {
            instance_context: Some(instance_context),
            ..self
        }
    }
}

/// Initializes tracing with stderr output only, without file or OpenTelemetry layers.
///
/// Intended for short-lived commands (e.g. verify) that must not write to the running AC log files.
pub fn try_init_stderr_tracing(config: &LoggingConfig) -> Result<(), TracingError> {
    let layers = vec![stderr(config)?];
    try_init_tracing_subscriber(layers)
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
    // Currently stderr output is always on, we could consider allowing to turn it off.
    let mut layers = Vec::from([stderr(&config.logging_config)?]);
    let mut guards = Vec::<TracingGuardBox>::new();

    if let Some((file_layer, file_guard)) = file(&config.logging_config, config.logging_path)? {
        layers.push(file_layer);
        guards.push(Box::new(file_guard));
    }

    if let Some(otel_config) = config.instrumentation_config.opentelemetry.as_ref() {
        tracing::debug!("opentelemetry config found, initializing OTLP layers");
        // Normalize headers (api_key -> api-key) for OTLP compatibility
        let normalized_config = otel_config.clone().normalize_headers();
        let (otel_layers, otel_guard) =
            OtelLayers::try_build(&normalized_config, config.instance_context.as_ref())?;
        layers.push(otel_layers);
        guards.push(Box::new(otel_guard));

        // Allows including the log information on spans that contain them when send to otlp.
        opentelemetry::global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );
    } else {
        tracing::debug!("no opentelemetry config found - self-instrumentation disabled");
    }
    let otel_enabled = config.instrumentation_config.opentelemetry.is_some();
    try_init_tracing_subscriber(layers)?;
    debug!("tracing_subscriber initialized successfully");
    tracing::info!(
        self_instrumentation_otlp_enabled = otel_enabled,
        "tracing initialized"
    );

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
