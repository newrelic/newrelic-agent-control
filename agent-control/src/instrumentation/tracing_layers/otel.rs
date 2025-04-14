use crate::http::client::{HttpBuildError, HttpClient};
use crate::http::config::HttpConfig;
use crate::instrumentation::config::otel::OtelConfig;
use crate::instrumentation::tracing::LayerBox;
use opentelemetry::trace::TracerProvider;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_http::HttpClient as OtelHttpClient;
use opentelemetry_otlp::{ExporterBuildError, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::logs::{BatchLogProcessor, SdkLoggerProvider};
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::{BatchSpanProcessor, SdkTracerProvider};
use opentelemetry_sdk::Resource;
use std::sync::LazyLock;
use thiserror::Error;
use tracing_opentelemetry::MetricsLayer;
use tracing_subscriber::{EnvFilter, Layer};

const TRACER_NAME: &str = "agent-control-self-instrumentation";

static RESOURCE: LazyLock<Resource> =
    LazyLock::new(|| Resource::builder().with_service_name(TRACER_NAME).build());

/// Enumerates the possible error building OpenTelemetry providers.
#[derive(Debug, Error)]
pub enum OtelBuildError {
    #[error("could not build the otel http client: {0}")]
    HttpClient(#[from] HttpBuildError),
    #[error("could not build the exporter: {0}")]
    ExporterBuild(#[from] ExporterBuildError),
    #[error("invalid filtering directive `{directive}`: {err}")]
    FilteringDirective { directive: String, err: String },
}

/// Holds the OpenTelemetry providers to report instrumentation. These providers will be used to
/// build the corresponding tracing layers.
///
/// The providers' shutdown will be automatically triggered when all their references are dropped.
/// Check the providers documentation for details. Eg: [SdkTracerProvider].
pub struct OtelLayers {
    traces_provider: Option<SdkTracerProvider>,
    metrics_provider: Option<SdkMeterProvider>,
    logs_provider: Option<SdkLoggerProvider>,
    filter: EnvFilter,
}

impl OtelLayers {
    /// Returns the [tracing_subscriber] layers corresponding to the provided configuration.
    pub fn try_build(config: &OtelConfig) -> Result<LayerBox, OtelBuildError> {
        let http_config = HttpConfig::new(
            config.client_timeout.clone().into(),
            config.client_timeout.clone().into(),
            config.proxy.clone(),
        );
        let http_client = HttpClient::new(http_config)?;
        let otel_layers = OtelLayers::try_new_with_client(config, http_client)?;
        Ok(otel_layers.layers())
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
            .then(|| Self::metrics_provider(client.clone(), config))
            .transpose()?;

        let logs_provider = config
            .logs
            .enabled
            .then(|| Self::logs_provider(client, config))
            .transpose()?;

        let filter = EnvFilter::builder()
            .parse(&config.insecure_level)
            .map_err(|err| OtelBuildError::FilteringDirective {
                directive: config.insecure_level.clone(),
                err: err.to_string(),
            })?;

        Ok(Self {
            traces_provider,
            metrics_provider,
            logs_provider,
            filter,
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

    fn logs_provider<C>(client: C, config: &OtelConfig) -> Result<SdkLoggerProvider, OtelBuildError>
    where
        C: OtelHttpClient + Send + Sync + 'static,
    {
        let exporter = opentelemetry_otlp::LogExporter::builder()
            .with_http()
            .with_http_client(client)
            .with_endpoint(config.logs_endpoint())
            .with_headers(config.headers.clone())
            .build()?;

        let batch_processor = BatchLogProcessor::builder(exporter)
            .with_batch_config((&config.logs.batch_config).into())
            .build();

        Ok(SdkLoggerProvider::builder()
            .with_log_processor(batch_processor)
            .with_resource(RESOURCE.clone())
            .build())
    }

    /// Return the layers for [tracing_subscriber] corresponding to the enabled OpenTelemetry providers.
    pub fn layers(self) -> LayerBox {
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
        if let Some(logs_provider) = self.logs_provider.as_ref() {
            let layer = OpenTelemetryTracingBridge::new(logs_provider);
            layers.push(Box::new(layer));
        }

        layers.with_filter(self.filter).boxed()
    }
}

#[cfg(test)]
mod tests {
    use http::Response;
    use tracing::{debug, info, trace};
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::EnvFilter;

    use crate::http::client::tests::MockOtelHttpClientMock;
    use crate::instrumentation::config::otel::{LogsConfig, OtelConfig};
    use crate::instrumentation::tracing_layers::otel::OtelLayers;

    #[test]
    fn test_logs_layer() {
        const INFO_LOG: &str = "foo";
        const DEBUG_LOG: &str = "bar";
        const TRACE_LOG: &str = "baz";

        let mut mock_http_client = MockOtelHttpClientMock::new();
        // Asserts info logs are sent by otlp exporter
        mock_http_client
            .expect_send_bytes()
            .once()
            .withf(|req| {
                let body = String::from_utf8_lossy(req.body().as_ref());
                req.uri().path().eq("/v1/logs")
                    && body.contains(INFO_LOG)
                    && !body.contains(DEBUG_LOG)
                    && !body.contains(TRACE_LOG)
            })
            .returning(|_| {
                Ok(Response::builder()
                    .status(200)
                    .body(opentelemetry_http::Bytes::default())
                    .unwrap())
            });

        let logs_provider = OtelLayers::logs_provider(
            mock_http_client,
            &OtelConfig {
                logs: LogsConfig {
                    enabled: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();

        let otel_providers = OtelLayers {
            logs_provider: Some(logs_provider),
            filter: EnvFilter::builder().parse_lossy("info"),
            traces_provider: None,
            metrics_provider: None,
        };

        let subscriber = tracing_subscriber::Registry::default().with(otel_providers.layers());
        tracing::subscriber::with_default(subscriber, || {
            info!(INFO_LOG);
            debug!(DEBUG_LOG);
            trace!(TRACE_LOG);
        });
    }
}
