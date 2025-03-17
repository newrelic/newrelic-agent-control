use super::config::OtelConfig;
use crate::instrumentation::tracing::LayerBox;
use opentelemetry::global;
use opentelemetry::trace::{TraceError, TracerProvider};
use opentelemetry_http::HttpClient;
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::error::OTelSdkError;
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
pub enum OtelProviderBuildError {
    #[error("could not build traces exporter: {0}")]
    Traces(#[from] TraceError),
    #[error("could not build metrics exporter: {0}")]
    Metrics(#[from] MetricError),
}

/// Error shutting down the OpenTelemetry providers.
pub type OtelShutdownError = OTelSdkError;

/// Holds the OpenTelemetry providers to report instrumentation.
pub struct OtelProviders {
    traces_provider: Option<SdkTracerProvider>,
    metrics_provider: Option<SdkMeterProvider>,
}

impl OtelProviders {
    /// Builds the providers corresponding to the provided configuration.
    pub fn try_new<C>(config: &OtelConfig, client: C) -> Result<Self, OtelProviderBuildError>
    where
        C: HttpClient + Send + Sync + Clone + 'static,
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
    ) -> Result<SdkTracerProvider, OtelProviderBuildError>
    where
        C: HttpClient + Send + Sync + 'static,
    {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_http_client(client)
            .with_endpoint(config.endpoint.to_string())
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
    ) -> Result<SdkMeterProvider, OtelProviderBuildError>
    where
        C: HttpClient + Send + Sync + 'static,
    {
        let exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_http_client(client)
            .with_endpoint(config.endpoint.to_string())
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

    /// Set the configured providers as global providers, check [opentelemetry::global] for details.
    pub fn set_global(&self) {
        if let Some(traces_provider) = self.traces_provider.as_ref() {
            global::set_tracer_provider(traces_provider.clone());
        }
        if let Some(metrics_provider) = self.metrics_provider.as_ref() {
            global::set_meter_provider(metrics_provider.clone());
        }
    }

    /// Shuts down the configured providers.
    pub fn shutdown(&self) -> Result<(), OtelShutdownError> {
        if let Some(traces_provider) = self.traces_provider.as_ref() {
            traces_provider.shutdown()?;
        }
        if let Some(metrics_provider) = self.metrics_provider.as_ref() {
            metrics_provider.shutdown()?;
        }
        Ok(())
    }

    /// Return the layers to be used with [tracing_opentelemetry] corresponding to the enabled OpenTelemetry providers.
    pub fn tracing_layers(&self) -> Vec<LayerBox> {
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
