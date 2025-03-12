use opentelemetry::global;
use opentelemetry::trace::TracerProvider;
use opentelemetry_http::HttpClient;
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use std::sync::LazyLock;
use tracing_opentelemetry::MetricsLayer;

use crate::instrumentation::tracing::LayerBox;

use super::config::OtelConfig;

static RESOURCE: LazyLock<Resource> = LazyLock::new(|| {
    Resource::builder()
        .with_service_name("agent-control-self-instrumentation")
        .build()
});

const OTLP_ENDPOINT: &str = "https://otlp.nr-data.net";

// TODO: get rid of the client and keep only providers (in order to shut them down and set them globally)
// also avoid to set them globally before initialization
pub struct OtelProviders {
    traces_provider: Option<SdkTracerProvider>,
    metrics_provider: Option<SdkMeterProvider>,
}

impl OtelProviders {
    pub fn new<C>(config: &OtelConfig, client: C) -> Self
    where
        C: HttpClient + Send + Sync + Clone + 'static,
    {
        Self {
            traces_provider: config.traces.then(|| Self::traces_provider(client.clone())),
            metrics_provider: config.metrics.then(|| Self::metrics_provider(client)),
        }
    }

    fn traces_provider<C>(client: C) -> SdkTracerProvider
    where
        C: HttpClient + Send + Sync + 'static,
    {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_http_client(client)
            .with_endpoint(OTLP_ENDPOINT)
            .build()
            .expect("Could not build HTTP exporter"); // TODO: handle error

        SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(RESOURCE.clone())
            .build()
    }

    fn metrics_provider<C>(client: C) -> SdkMeterProvider
    where
        C: HttpClient + Send + Sync + 'static,
    {
        let exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_http_client(client)
            .with_endpoint(OTLP_ENDPOINT)
            .build()
            .expect("Could not build HTTP exporter"); // TODO: handle error

        SdkMeterProvider::builder()
            .with_periodic_exporter(exporter)
            .with_resource(RESOURCE.clone())
            .build()
    }

    pub fn set_global(&self) {
        if let Some(traces_provider) = self.traces_provider.as_ref() {
            global::set_tracer_provider(traces_provider.clone());
        }
        if let Some(metrics_provider) = self.metrics_provider.as_ref() {
            global::set_meter_provider(metrics_provider.clone());
        }
    }

    pub fn shutdown(&self) -> OTelSdkResult {
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

    // TODO: proper names
    pub fn tracing_layers(&self) -> Vec<LayerBox> {
        let mut layers = Vec::<LayerBox>::new();
        if let Some(traces_provider) = self.traces_provider.as_ref() {
            let layer = tracing_opentelemetry::layer().with_tracer(traces_provider.tracer("..."));
            layers.push(Box::new(layer));
        }
        if let Some(metrics_provider) = self.metrics_provider.as_ref() {
            let layer = MetricsLayer::new(metrics_provider.clone());
            layers.push(Box::new(layer));
        }
        layers
    }
}
