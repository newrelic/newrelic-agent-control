use opentelemetry::global;
use opentelemetry::trace::{TraceError, TracerProvider};
use opentelemetry_http::HttpClient;
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::error::OTelSdkError;
use opentelemetry_sdk::metrics::{MetricError, SdkMeterProvider};
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use std::sync::LazyLock;
use thiserror::Error;
use tracing_opentelemetry::MetricsLayer;

use crate::instrumentation::tracing::LayerBox;

use super::config::OtelConfig;

const TRACER_NAME: &str = "agent-control-self-instrumentation";

static RESOURCE: LazyLock<Resource> =
    LazyLock::new(|| Resource::builder().with_service_name(TRACER_NAME).build());

const OTLP_ENDPOINT: &str = "https://otlp.nr-data.net";

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
            .then(|| Self::traces_provider(client.clone()))
            .transpose()?;

        let metrics_provider = config
            .metrics
            .then(|| Self::metrics_provider(client))
            .transpose()?;

        Ok(Self {
            traces_provider,
            metrics_provider,
        })
    }

    fn traces_provider<C>(client: C) -> Result<SdkTracerProvider, OtelProviderBuildError>
    where
        C: HttpClient + Send + Sync + 'static,
    {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_http_client(client)
            .with_endpoint(OTLP_ENDPOINT)
            .build()?;

        Ok(SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(RESOURCE.clone())
            .build())
    }

    fn metrics_provider<C>(client: C) -> Result<SdkMeterProvider, OtelProviderBuildError>
    where
        C: HttpClient + Send + Sync + 'static,
    {
        let exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_http_client(client)
            .with_endpoint(OTLP_ENDPOINT)
            .build()?;

        Ok(SdkMeterProvider::builder()
            .with_periodic_exporter(exporter)
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
        self.traces_provider
            .as_ref()
            .map(SdkTracerProvider::shutdown)
            .transpose()?;
        self.metrics_provider
            .as_ref()
            .map(SdkMeterProvider::shutdown)
            .transpose()?;
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
