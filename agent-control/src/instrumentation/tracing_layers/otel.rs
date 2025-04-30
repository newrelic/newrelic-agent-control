use crate::http::client::{HttpBuildError, HttpClient};
use crate::http::config::HttpConfig;
use crate::instrumentation::config::otel::OtelConfig;
use crate::instrumentation::tracing::{LayerBox, TracingGuard};
use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_http::HttpClient as OtelHttpClient;
use opentelemetry_otlp::{ExporterBuildError, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::{BatchLogProcessor, SdkLoggerProvider};
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::{BatchSpanProcessor, SdkTracerProvider};
use thiserror::Error;
use tracing_opentelemetry::MetricsLayer;
use tracing_subscriber::{EnvFilter, Layer};

const TRACER_NAME: &str = "agent-control-self-instrumentation";

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

/// Holds the resources to build the layers for [tracing_subscriber] that will allow reporting telemetry
/// through OpenTelemetry.
///
/// The underlying OpenTelemetry providers will be automatically shutdown when all their references are dropped.
/// Therefore, in order to keep the reference for as long as needed, a guard is returned with the layers.
/// For more information about automatic shutting down the OpenTelemetry providers, check the providers documentation.
/// Eg: [SdkTracerProvider].
#[derive(Default)]
pub struct OtelLayers {
    logs_layer_builder: Option<(SdkLoggerProvider, EnvFilter)>,
    traces_layer_builder: Option<(SdkTracerProvider, EnvFilter)>,
    // Metrics are reported regardless of the configured level, there are no filtering options supported for now.
    metrics_layer_builder: Option<SdkMeterProvider>,
}

impl OtelLayers {
    /// Returns the layers for [tracing_subscriber] corresponding to the enabled OpenTelemetry providers and the corresponding
    /// _guard_ that needs to be keep alive in order to avoid shutting down the corresponding exporters while telemetry
    /// is emitted. When the _guard_ is dropped all the exporters are shut down and the remaining telemetry is sent.
    pub fn try_build(config: &OtelConfig) -> Result<(LayerBox, OtelGuard), OtelBuildError> {
        let http_config = HttpConfig::new(
            config.client_timeout.clone().into(),
            config.client_timeout.clone().into(),
            config.proxy.clone(),
        );
        let http_client = HttpClient::new(http_config)?;
        let otel_layers = OtelLayers::try_new_with_client(config, http_client)?;
        Ok(otel_layers.layers())
    }

    /// Builds the providers and filters corresponding to the provided configuration.
    pub(crate) fn try_new_with_client<C>(
        config: &OtelConfig,
        client: C,
    ) -> Result<Self, OtelBuildError>
    where
        C: OtelHttpClient + Send + Sync + Clone + 'static,
    {
        if !(config.traces.enabled || config.metrics.enabled || config.logs.enabled) {
            return Ok(Self::default());
        }

        // Set up the resource and custom attributes
        let attributes: Vec<KeyValue> = config
            .custom_attributes
            .iter()
            .map(|(k, v)| KeyValue::new(k.clone(), v.clone()))
            .collect();

        let resource = Resource::builder()
            .with_service_name(TRACER_NAME)
            .with_attributes(attributes)
            .build();

        // Build each layer if configured
        let traces_layer_builder = if config.traces.enabled {
            Some((
                Self::traces_provider(client.clone(), config, resource.clone())?,
                Self::filter(&config.insecure_level)?,
            ))
        } else {
            None
        };

        let metrics_layer_builder = if config.metrics.enabled {
            Some(Self::metrics_provider(
                client.clone(),
                config,
                resource.clone(),
            )?)
        } else {
            None
        };

        let logs_layer_builder = if config.logs.enabled {
            Some((
                Self::logs_provider(client, config, resource)?,
                Self::filter(&config.insecure_level)?,
            ))
        } else {
            None
        };

        Ok(Self {
            logs_layer_builder,
            metrics_layer_builder,
            traces_layer_builder,
        })
    }

    fn filter(insecure_level: &str) -> Result<EnvFilter, OtelBuildError> {
        EnvFilter::builder().parse(insecure_level).map_err(|err| {
            OtelBuildError::FilteringDirective {
                directive: insecure_level.to_string(),
                err: err.to_string(),
            }
        })
    }

    fn traces_provider<C>(
        client: C,
        config: &OtelConfig,
        resource: Resource,
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
            .with_resource(resource)
            .build())
    }

    fn metrics_provider<C>(
        client: C,
        config: &OtelConfig,
        resource: Resource,
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
            .with_resource(resource)
            .build())
    }

    fn logs_provider<C>(
        client: C,
        config: &OtelConfig,
        resource: Resource,
    ) -> Result<SdkLoggerProvider, OtelBuildError>
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
            .with_resource(resource)
            .build())
    }

    pub fn layers(self) -> (LayerBox, OtelGuard) {
        let mut layers = Vec::<LayerBox>::new();
        let mut guard = OtelGuard::default();

        if let Some((traces_provider, traces_filter)) = self.traces_layer_builder {
            guard._traces_provider = Some(traces_provider.clone());
            let layer =
                tracing_opentelemetry::layer().with_tracer(traces_provider.tracer(TRACER_NAME));
            layers.push(Box::new(layer.with_filter(traces_filter)));
        }

        if let Some(metrics_provider) = self.metrics_layer_builder {
            guard._metrics_provider = Some(metrics_provider.clone());
            let layer = MetricsLayer::new(metrics_provider.clone());
            layers.push(Box::new(layer));
        }

        if let Some((logs_provider, logs_filter)) = self.logs_layer_builder {
            guard._logs_provider = Some(logs_provider.clone());
            let layer = OpenTelemetryTracingBridge::new(&logs_provider);
            layers.push(Box::new(layer.with_filter(logs_filter)));
        }

        (layers.boxed(), guard)
    }
}

/// Keeps a reference to the OpenTelemetry providers to avoid shutting down the underlying reporters while telemetry
/// is emitted.
#[derive(Default)]
pub struct OtelGuard {
    _logs_provider: Option<SdkLoggerProvider>,
    _metrics_provider: Option<SdkMeterProvider>,
    _traces_provider: Option<SdkTracerProvider>,
}

impl TracingGuard for OtelGuard {}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http::Response;
    use opentelemetry_sdk::Resource;
    use tracing::{debug, info, trace};
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::layer::SubscriberExt;

    use crate::http::client::tests::MockOtelHttpClient;
    use crate::instrumentation::config::otel::{LogsConfig, MetricsConfig, OtelConfig};
    use crate::instrumentation::tracing_layers::otel::OtelLayers;

    #[test]
    fn test_logs_layer() {
        const INFO_LOG: &str = "foo";
        const DEBUG_LOG: &str = "bar";
        const TRACE_LOG: &str = "baz";

        let mut mock_http_client = MockOtelHttpClient::new();
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
            Resource::builder().build(),
        )
        .unwrap();

        let otel_providers = OtelLayers {
            logs_layer_builder: Some((logs_provider, EnvFilter::builder().parse_lossy("info"))),
            ..Default::default()
        };

        let (layers, _guard) = otel_providers.layers();
        let subscriber = tracing_subscriber::Registry::default().with(layers);
        tracing::subscriber::with_default(subscriber, || {
            info!(INFO_LOG);
            debug!(DEBUG_LOG);
            trace!(TRACE_LOG);
        });
    }

    #[test]
    fn test_metrics_layer() {
        let mut mock_http_client = MockOtelHttpClient::new();
        // Asserts metrics are sent
        mock_http_client
            .expect_send_bytes()
            .times(1..) // The metric should be sent at least once
            .withf(|req| {
                let body = String::from_utf8_lossy(req.body().as_ref());
                req.uri().path().eq("/v1/metrics") && body.contains("uptime")
            })
            .returning(|_| {
                Ok(Response::builder()
                    .status(200)
                    .body(opentelemetry_http::Bytes::default())
                    .unwrap())
            });

        let metrics_provider = OtelLayers::metrics_provider(
            mock_http_client,
            &OtelConfig {
                metrics: MetricsConfig {
                    enabled: true,
                    interval: Duration::from_secs(1).into(),
                },
                ..Default::default()
            },
            Resource::builder().build(),
        )
        .unwrap();

        let otel_layers = OtelLayers {
            metrics_layer_builder: Some(metrics_provider),
            ..Default::default()
        };
        let (layers, _guard) = otel_layers.layers();
        let subscriber = tracing_subscriber::Registry::default().with(layers);
        tracing::subscriber::with_default(subscriber, || {
            trace!(monotonic_counter.uptime = 42);
            std::thread::sleep(Duration::from_secs(2));
        });
    }
}
