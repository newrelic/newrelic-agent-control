use crate::http::client::{HttpBuildError, HttpClient};
use crate::http::config::HttpConfig;
use crate::instrumentation::config::otel::OtelConfig;
use crate::instrumentation::tracing::LayerBox;
use opentelemetry::trace::{TraceError, TracerProvider};
use opentelemetry_http::HttpClient as OtelHttpClient;
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::metrics::{MetricError, PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::{BatchSpanProcessor, SdkTracerProvider};
use opentelemetry_sdk::Resource;
use std::sync::LazyLock;
use thiserror::Error;
use tracing_opentelemetry::MetricsLayer;

const TRACER_NAME: &str = "agent-control-self-instrumentation";

static RESOURCE: LazyLock<Resource> =
    LazyLock::new(|| Resource::builder().with_service_name(TRACER_NAME).build());

/// Enumerates the possible error building OpenTelemetry providers.
#[derive(Debug, Error)]
pub enum OtelBuildError {
    #[error("could not build the otel http client: {0}")]
    HttpClient(#[from] HttpBuildError),
    #[error("could not build traces exporter: {0}")]
    Traces(#[from] TraceError),
    #[error("could not build metrics exporter: {0}")]
    Metrics(#[from] MetricError),
}

/// Holds the OpenTelemetry providers to report instrumentation. These providers will be used to
/// build the corresponding tracing layers.
///
/// The providers' shutdown will be automatically triggered when all their references are dropped.
/// Check the providers documentation for details. Eg: [SdkTracerProvider].
pub struct OtelLayers {
    traces_provider: Option<SdkTracerProvider>,
    metrics_provider: Option<SdkMeterProvider>,
}

impl OtelLayers {
    /// Returns the [tracing_subscriber] layers corresponding to the provided configuration.
    pub fn try_build(config: &OtelConfig) -> Result<Vec<LayerBox>, OtelBuildError> {
        let http_config = HttpConfig::new(
            config.client_timeout.clone().into(),
            config.client_timeout.clone().into(),
            config.proxy.clone(),
        );
        let http_client = HttpClient::new(http_config)?;
        let otel_providers = OtelLayers::try_new_with_client(config, http_client)?;
        Ok(otel_providers.layers())
    }

    /// Builds the providers corresponding to the provided configuration.
    pub(crate) fn try_new_with_client<C>(
        config: &OtelConfig,
        client: C,
    ) -> Result<Self, OtelBuildError>
    where
        C: OtelHttpClient + Send + Sync + Clone + 'static,
    {
        let traces_provider = config
            .traces
            .enabled
            .then(|| Self::traces_provider(client.clone(), config))
            .transpose()?;

        let metrics_provider = config
            .metrics
            .enabled
            .then(|| Self::metrics_provider(client, config))
            .transpose()?;

        Ok(Self {
            traces_provider,
            metrics_provider,
        })
    }

    fn traces_provider<C>(
        client: C,
        config: &OtelConfig,
    ) -> Result<SdkTracerProvider, OtelBuildError>
    where
        C: OtelHttpClient + Send + Sync + 'static,
    {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_http_client(client)
            .with_endpoint(config.traces_endpoint().to_string())
            .with_headers(config.headers.clone())
            .build()?;

        let batch_processor = BatchSpanProcessor::builder(exporter)
            .with_batch_config((&config.traces.batch_config).into())
            .build();

        Ok(SdkTracerProvider::builder()
            .with_span_processor(batch_processor)
            .with_resource(RESOURCE.clone())
            .build())
    }

    fn metrics_provider<C>(
        client: C,
        config: &OtelConfig,
    ) -> Result<SdkMeterProvider, OtelBuildError>
    where
        C: OtelHttpClient + Send + Sync + 'static,
    {
        let exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_http_client(client)
            .with_endpoint(config.metrics_endpoint().to_string())
            .with_headers(config.headers.clone())
            .build()?;

        let periodic_reader = PeriodicReader::builder(exporter)
            .with_interval(config.metrics.interval.clone().into())
            .build();

        Ok(SdkMeterProvider::builder()
            .with_reader(periodic_reader)
            .with_resource(RESOURCE.clone())
            .build())
    }

    /// Return the layers for [tracing_subscriber] corresponding to the enabled OpenTelemetry providers.
    pub fn layers(self) -> Vec<LayerBox> {
        let mut layers = Vec::<LayerBox>::new();
        if let Some(traces_provider) = self.traces_provider.as_ref() {
            let layer =
                tracing_opentelemetry::layer().with_tracer(traces_provider.tracer(TRACER_NAME));
            layers.push(Box::new(layer));
        }
        if let Some(metrics_provider) = self.metrics_provider.as_ref() {
            let layer = MetricsLayer::new(metrics_provider.clone());
            layers.push(Box::new(layer));
        }
        layers
    }
}
