//! Builds the [`tracing_subscriber`] layers that report logs and metrics through OpenTelemetry.

use crate::agent_control::defaults::{
    AGENT_CONTROL_VERSION, FLEET_ID_ATTRIBUTE_KEY, OS_ATTRIBUTE_KEY, OS_ATTRIBUTE_VALUE,
};
use crate::http::client::{HttpBuildError, HttpClient};
use crate::http::config::HttpConfig;
use crate::instrumentation::config::otel::OtelConfig;
use crate::instrumentation::flush::{FLUSH_TIMEOUT, force_flush_with_timeout};
use crate::instrumentation::metrics as ac_metrics;
use crate::instrumentation::tracing::{InstanceContext, LayerBox, TracingGuard};
use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider;
use opentelemetry_appender_tracing::layer::{OpenTelemetryTracingBridge, TracingSpanAttributes};
use opentelemetry_http::HttpClient as OtelHttpClient;
use opentelemetry_otlp::{ExporterBuildError, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::{BatchLogProcessor, SdkLoggerProvider};
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider, Temporality};
use opentelemetry_sdk::trace::{BatchSpanProcessor, SdkTracerProvider};
use resource_detection::Detector;
use resource_detection::system::detector::SystemDetector;
use resource_detection::system::hostname::get_hostname;
use thiserror::Error;
use tracing_opentelemetry::MetricsLayer;
use tracing_subscriber::{EnvFilter, Layer};

/// Prefix for the service.name OTLP attribute.
/// The full name is `SERVICE_NAME_PREFIX-<hostname>` so each AC instance
/// appears as a distinct service in NR (e.g. `agent-control-my-host`).
const SERVICE_NAME_PREFIX: &str = "agent-control";

/// Enumerates the possible error building OpenTelemetry providers.
#[derive(Debug, Error)]
pub enum OtelBuildError {
    /// The OpenTelemetry HTTP client could not be built.
    #[error("could not build the otel http client: {0}")]
    HttpClient(#[from] HttpBuildError),
    /// An OpenTelemetry exporter could not be built.
    #[error("could not build the exporter: {0}")]
    ExporterBuild(#[from] ExporterBuildError),
    /// A filtering directive could not be parsed.
    #[error("invalid filtering directive '{directive}': {err}")]
    FilteringDirective {
        /// The directive that failed to parse.
        directive: String,
        /// The underlying parsing error.
        err: String,
    },
}

/// Holds the resources to build the layers for [tracing_subscriber] that will allow reporting telemetry
/// through OpenTelemetry.
///
/// The underlying OpenTelemetry providers will be automatically shutdown when all their references are dropped.
/// Therefore, in order to keep the reference for as long as needed, a guard is returned with the layers.
/// For more information about automatic shutting down the OpenTelemetry providers, check the providers documentation.
/// Eg: [SdkLoggerProvider].
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
    pub fn try_build(
        config: &OtelConfig,
        instance_context: Option<&InstanceContext>,
    ) -> Result<(LayerBox, OtelGuard), OtelBuildError> {
        tracing::debug!(
            metrics_enabled = config.metrics.enabled,
            traces_enabled = config.traces.enabled,
            logs_enabled = config.logs.enabled,
            endpoint = %config.endpoint,
            "otel layers build started"
        );

        let http_config = HttpConfig::new(
            config.client_timeout.clone().into(),
            config.client_timeout.clone().into(),
            config.proxy.clone(),
        );
        let http_client = HttpClient::new(http_config)?;
        let otel_layers = OtelLayers::try_new_with_client(config, instance_context, http_client)?;
        Ok(otel_layers.layers())
    }

    /// Builds the providers and filters corresponding to the provided configuration.
    pub(crate) fn try_new_with_client<C>(
        config: &OtelConfig,
        instance_context: Option<&InstanceContext>,
        client: C,
    ) -> Result<Self, OtelBuildError>
    where
        C: OtelHttpClient + Send + Sync + Clone + 'static,
    {
        if !(config.traces.enabled || config.metrics.enabled || config.logs.enabled) {
            tracing::debug!(
                metrics_enabled = config.metrics.enabled,
                traces_enabled = config.traces.enabled,
                logs_enabled = config.logs.enabled,
                "all telemetry disabled - returning empty otel layers"
            );
            return Ok(Self::default());
        }

        // Set up the resource and custom attributes
        let mut attributes: Vec<KeyValue> = config
            .custom_attributes
            .iter()
            .map(|(k, v)| KeyValue::new(k.clone(), v.clone()))
            .collect();

        // host.name / service.instance.id: use a proper syscall instead of an env var so the
        // value is always accurate regardless of whether $HOSTNAME is set in the environment.
        // The hostname is also used to build the service.name below.
        let hostname = match get_hostname() {
            Ok(h) => {
                tracing::debug!(host_name = %h, "added host.name");
                attributes.push(KeyValue::new("host.name", h.clone()));
                attributes.push(KeyValue::new("service.instance.id", h.clone()));
                Some(h)
            }
            Err(e) => {
                tracing::warn!(error = %e, "could not detect hostname for OTLP resource attributes");
                None
            }
        };

        // host.id: stable machine identifier from /etc/machine-id (Linux) or registry (Windows).
        // Allows distinguishing two machines with the same hostname.
        match SystemDetector::default().detect() {
            Ok(resource) => {
                if let Some(machine_id) = resource.get(resource_detection::Key::from(
                    resource_detection::system::MACHINE_ID_KEY,
                )) {
                    let machine_id = String::from(machine_id);
                    tracing::debug!(host_id = %machine_id, "added host.id");
                    attributes.push(KeyValue::new("host.id", machine_id));
                } else {
                    // Non-fatal: host.id not available on all platforms (e.g. some container envs)
                    tracing::debug!("host.id not available, skipping");
                }
            }
            Err(e) => {
                // Non-fatal: host.id not available on all platforms (e.g. some container envs)
                tracing::debug!(error = %e, "host.id not available, skipping");
            }
        }

        attributes.extend(static_resource_attributes(instance_context));

        // service.name: "agent-control-<hostname>" so each instance is a distinct
        // service in NR. Falls back to the prefix alone when hostname is unavailable.
        let service_name = hostname
            .as_deref()
            .map(|h| format!("{SERVICE_NAME_PREFIX}-{h}"))
            .unwrap_or_else(|| SERVICE_NAME_PREFIX.to_string());

        let resource = Resource::builder()
            .with_service_name(service_name)
            .with_attributes(attributes)
            .build();

        // Build each layer if configured
        let traces_layer_builder = if config.traces.enabled {
            tracing::debug!(endpoint = %config.traces_endpoint(), "building traces provider");
            Some((
                Self::traces_provider(client.clone(), config, resource.clone())?,
                Self::filter(&config.insecure_level)?,
            ))
        } else {
            tracing::debug!("traces disabled, skipping traces provider");
            None
        };

        let metrics_layer_builder = if config.metrics.enabled {
            tracing::debug!(endpoint = %config.metrics_endpoint(), "building metrics provider");
            Some(Self::metrics_provider(
                client.clone(),
                config,
                resource.clone(),
            )?)
        } else {
            tracing::debug!("metrics disabled, skipping metrics provider");
            None
        };

        let logs_layer_builder = if config.logs.enabled {
            tracing::debug!(endpoint = %config.logs_endpoint(), "building logs provider");
            Some((
                Self::logs_provider(client, config, resource)?,
                Self::filter(&config.insecure_level)?,
            ))
        } else {
            tracing::debug!("logs disabled, skipping logs provider");
            None
        };

        Ok(Self {
            logs_layer_builder,
            traces_layer_builder,
            metrics_layer_builder,
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
            // The SDK defaults to Cumulative temporality, which reports each counter's
            // running total since process start on every export tick instead of the
            // delta since the last tick. NRQL's sum() expects deltas (that's what makes
            // it additive across a time window) - left at the default, every counter in
            // this module reports data NRQL can count() but not meaningfully sum().
            .with_temporality(Temporality::Delta)
            .build()?;

        let periodic_reader = PeriodicReader::builder(exporter)
            .with_interval(config.metrics.interval.clone().into())
            .build();

        let provider = SdkMeterProvider::builder()
            .with_reader(periodic_reader)
            .with_resource(resource)
            .build();

        // Expose via the global OTel API so call sites throughout the binary
        // can use `opentelemetry::global::meter("agent-control")` without
        // holding a direct reference to the provider.
        opentelemetry::global::set_meter_provider(provider.clone());
        // Register in the metrics module so flush() can call force_flush()
        // before process self-replacement (exec-based restart bypasses Drop).
        ac_metrics::register_provider(provider.clone());

        Ok(provider)
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

    /// Consumes the providers and returns the combined [`LayerBox`] together with the [`OtelGuard`]
    /// that must be kept alive while telemetry is emitted.
    pub fn layers(self) -> (LayerBox, OtelGuard) {
        let mut layers = Vec::<LayerBox>::new();
        let mut guard = OtelGuard::default();

        if let Some((traces_provider, traces_filter)) = self.traces_layer_builder {
            tracing::debug!("creating traces layer");
            guard._traces_provider = Some(traces_provider.clone());
            let layer = tracing_opentelemetry::layer()
                .with_tracer(traces_provider.tracer(SERVICE_NAME_PREFIX));
            layers.push(Box::new(layer.with_filter(traces_filter)));
        }

        if let Some(metrics_provider) = self.metrics_layer_builder {
            tracing::debug!("creating metrics layer");
            guard._metrics_provider = Some(metrics_provider.clone());

            // Prime the exporter: register a startup counter so the PeriodicReader
            // has at least one instrument and attempts an export on its first tick.
            // Without this, if no tracing events reach MetricsLayer before the first
            // tick, the PeriodicReader finds no instruments and skips the HTTP call.
            // Use u64_counter (monotonic) not UpDownCounter — NR treats them differently.
            use opentelemetry::metrics::MeterProvider as _;
            let meter = metrics_provider.meter(SERVICE_NAME_PREFIX);
            let startup_counter = meter.u64_counter("agent_control.starts").build();
            startup_counter.add(1, &[]);
            // Store in guard so the instrument stays registered for the lifetime of the process.
            // Must be listed BEFORE _metrics_provider in OtelGuard so it's dropped first;
            // Rust drops fields in declaration order and the counter must be released
            // before the provider calls try_shutdown() to avoid a stale-instrument warning.
            guard._startup_counter = Some(startup_counter);

            let layer = MetricsLayer::new(metrics_provider.clone());
            layers.push(Box::new(
                layer.with_filter(tracing_subscriber::filter::LevelFilter::TRACE),
            ));
        }

        if let Some((logs_provider, logs_filter)) = self.logs_layer_builder {
            tracing::debug!("creating logs layer");
            guard._logs_provider = Some(logs_provider.clone());
            // Copy tracing-span attributes (e.g. the agent `id` set via `info_span!`)
            // onto every log record emitted within that span. Without this, logs
            // emitted deeper in a span (e.g. "Checking health") carry no agent
            // identity at all, since the bridge only visits the event's own fields
            // by default.
            let layer = OpenTelemetryTracingBridge::builder(&logs_provider)
                .with_tracing_span_attributes(TracingSpanAttributes::all())
                .build();
            layers.push(Box::new(layer.with_filter(logs_filter)));
        }

        tracing::debug!(layer_count = layers.len(), "OTLP layers created");
        (layers.boxed(), guard)
    }
}

/// Builds the deterministic (non-syscall-dependent) resource attributes: service/telemetry/
/// instrumentation semantic conventions, entity type, real OS, and instance-identifying context
/// (deployment platform, k8s cluster, fleet). Split out from `try_new_with_client` so these
/// attributes can be asserted directly in tests without mocking HTTP clients or providers.
fn static_resource_attributes(instance_context: Option<&InstanceContext>) -> Vec<KeyValue> {
    let mut attributes = vec![
        // Standard OpenTelemetry semantic conventions for service
        KeyValue::new("service.namespace", "newrelic"),
        KeyValue::new("service.version", AGENT_CONTROL_VERSION),
        // OpenTelemetry semantic conventions for telemetry SDK
        KeyValue::new("telemetry.sdk.name", "agent-control"),
        KeyValue::new("telemetry.sdk.language", "rust"),
        KeyValue::new("telemetry.sdk.version", AGENT_CONTROL_VERSION),
        // New Relic-specific entity and instrumentation attributes.
        // Matches the pattern used by Infrastructure Agent for dimensional metrics.
        KeyValue::new("instrumentation.provider", "newrelic"),
        KeyValue::new("instrumentation.name", "agent-control"),
        KeyValue::new("instrumentation.version", AGENT_CONTROL_VERSION),
        // New Relic entity type identification (kept as NRAgentControl per user request)
        KeyValue::new("newrelic.entity.type", "NRAgentControl"),
        KeyValue::new("entity.type", "NRAgentControl"),
        // Real OS the process runs on, same key/values OpAMP's agent_description already uses
        // (agent_control::defaults::OS_ATTRIBUTE_KEY/VALUE) - keeps OTel and OpAMP consistent.
        KeyValue::new(OS_ATTRIBUTE_KEY, OS_ATTRIBUTE_VALUE),
    ];

    // Instance-identifying context: deployment platform, k8s cluster, fleet. Lets multiple
    // concurrent AC instances be told apart and grouped in self-instrumentation telemetry.
    // `agent_control.environment` is deliberately distinct from os.type above: for k8s the
    // process still runs on Linux inside the pod, so the deployment platform ("kubernetes")
    // and the real OS ("linux") are different facts and shouldn't be conflated.
    if let Some(ctx) = instance_context {
        attributes.push(KeyValue::new(
            "agent_control.environment",
            ctx.environment.deployment_platform(),
        ));
        if let Some(cluster_name) = ctx.cluster_name.as_deref().filter(|s| !s.is_empty()) {
            attributes.push(KeyValue::new("k8s.cluster.name", cluster_name.to_string()));
        }
        if let Some(fleet_id) = ctx.fleet_id.as_deref().filter(|s| !s.is_empty()) {
            attributes.push(KeyValue::new(FLEET_ID_ATTRIBUTE_KEY, fleet_id.to_string()));
        }
        // k8s.pod.name / k8s.node.name: distinct from host.name, which is the pod's own OS
        // hostname (k8s sets it equal to the pod name) - without these, there's no attribute
        // for the underlying node a given instance runs on.
        if let Some(pod_name) = ctx.pod_name.as_deref().filter(|s| !s.is_empty()) {
            attributes.push(KeyValue::new("k8s.pod.name", pod_name.to_string()));
        }
        if let Some(node_name) = ctx.node_name.as_deref().filter(|s| !s.is_empty()) {
            attributes.push(KeyValue::new("k8s.node.name", node_name.to_string()));
        }
        tracing::debug!(
            environment = %ctx.environment,
            cluster_name = ctx.cluster_name.as_deref().unwrap_or_default(),
            fleet_id = ctx.fleet_id.as_deref().unwrap_or_default(),
            pod_name = ctx.pod_name.as_deref().unwrap_or_default(),
            node_name = ctx.node_name.as_deref().unwrap_or_default(),
            "added instance-identifying attributes"
        );
    }

    attributes
}

/// Keeps a reference to the OpenTelemetry providers to avoid shutting down the underlying reporters while telemetry
/// is emitted. When dropped, shuts down all providers in dependency order.
///
/// **Field drop order matters**: Rust drops fields in declaration order.
/// The startup counter must come first so it releases its instrument handle
/// before the provider calls try_shutdown(), avoiding a stale-instrument warning.
/// Traces/logs providers come before metrics so the MetricsLayer outlives them.
pub struct OtelGuard {
    /// Startup counter — drop first, before the provider shuts down.
    _startup_counter: Option<opentelemetry::metrics::Counter<u64>>,
    _traces_provider: Option<SdkTracerProvider>,
    _logs_provider: Option<SdkLoggerProvider>,
    _metrics_provider: Option<SdkMeterProvider>,
}

impl Default for OtelGuard {
    fn default() -> Self {
        Self {
            _startup_counter: None,
            _traces_provider: None,
            _logs_provider: None,
            _metrics_provider: None,
        }
    }
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        // Explicit shutdown in reverse-dependency order with error visibility.
        // Fields are dropped by Rust after this fn returns (in declaration order),
        // but the try_shutdown() calls here flush buffered telemetry synchronously.
        if let Some(p) = self._traces_provider.clone() {
            force_flush_with_timeout("traces", FLUSH_TIMEOUT, move || p.force_flush());
        }
        if let Some(p) = self._metrics_provider.clone() {
            force_flush_with_timeout("metrics", FLUSH_TIMEOUT, move || p.force_flush());
        }
        // Replace the global meter provider with a no-op so any record_* calls
        // that race shutdown silently drop rather than writing to a shut-down provider.
        opentelemetry::global::set_meter_provider(
            opentelemetry_sdk::metrics::SdkMeterProvider::default(),
        );
    }
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

    use crate::agent_control::defaults::{
        FLEET_ID_ATTRIBUTE_KEY, OS_ATTRIBUTE_KEY, OS_ATTRIBUTE_VALUE,
    };
    use crate::environment::Environment;
    use crate::http::client::tests::MockOtelHttpClient;
    use crate::instrumentation::config::otel::{LogsConfig, MetricsConfig, OtelConfig};
    use crate::instrumentation::tracing::InstanceContext;
    use crate::instrumentation::tracing_layers::otel::OtelLayers;
    use rstest::rstest;

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
                // Accept any metrics export — either the startup counter or the uptime trace metric
                req.uri().path().eq("/v1/metrics")
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

    /// Verify that building OtelLayers with host detection (get_hostname + SystemDetector)
    /// does not panic. The resource-detection crate has its own unit tests for the
    /// individual detectors; here we only assert that the happy-path wiring compiles and runs.
    #[test]
    fn test_try_new_with_client_host_attributes_does_not_panic() {
        let mut mock_http_client = MockOtelHttpClient::new();
        mock_http_client.expect_send_bytes().returning(|_| {
            Ok(Response::builder()
                .status(200)
                .body(opentelemetry_http::Bytes::default())
                .unwrap())
        });

        let result = OtelLayers::try_new_with_client(
            &OtelConfig {
                logs: LogsConfig {
                    enabled: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            None,
            mock_http_client,
        );

        assert!(
            result.is_ok(),
            "OtelLayers::try_new_with_client should not fail: {:?}",
            result.err()
        );
    }

    fn attribute_value(attrs: &[opentelemetry::KeyValue], key: &str) -> Option<String> {
        attrs
            .iter()
            .find(|kv| kv.key.as_str() == key)
            .map(|kv| kv.value.to_string())
    }

    #[test]
    fn test_static_resource_attributes_without_instance_context() {
        let attrs = super::static_resource_attributes(None);

        assert_eq!(
            attribute_value(&attrs, OS_ATTRIBUTE_KEY).as_deref(),
            Some(OS_ATTRIBUTE_VALUE)
        );
        assert_eq!(
            attribute_value(&attrs, "instrumentation.name").as_deref(),
            Some("agent-control")
        );
        // No instance context provided: none of the identifying attributes should be present.
        assert_eq!(attribute_value(&attrs, "agent_control.environment"), None);
        assert_eq!(attribute_value(&attrs, "k8s.cluster.name"), None);
        assert_eq!(attribute_value(&attrs, FLEET_ID_ATTRIBUTE_KEY), None);
    }

    #[rstest]
    #[case::linux(Environment::Linux, "host")]
    #[case::windows(Environment::Windows, "host")]
    #[case::kubernetes(Environment::K8s, "kubernetes")]
    fn test_static_resource_attributes_environment(
        #[case] environment: Environment,
        #[case] expected: &str,
    ) {
        let ctx = InstanceContext {
            environment,
            cluster_name: None,
            fleet_id: None,
            pod_name: None,
            node_name: None,
        };
        let attrs = super::static_resource_attributes(Some(&ctx));

        assert_eq!(
            attribute_value(&attrs, "agent_control.environment").as_deref(),
            Some(expected)
        );
        // Real OS is always linux/windows/darwin per compile target, regardless of the
        // deployment-platform value above - the two attributes are independent.
        assert_eq!(
            attribute_value(&attrs, OS_ATTRIBUTE_KEY).as_deref(),
            Some(OS_ATTRIBUTE_VALUE)
        );
        // No cluster/fleet were set on this context.
        assert_eq!(attribute_value(&attrs, "k8s.cluster.name"), None);
        assert_eq!(attribute_value(&attrs, FLEET_ID_ATTRIBUTE_KEY), None);
    }

    #[test]
    fn test_static_resource_attributes_k8s_cluster_and_fleet() {
        let ctx = InstanceContext {
            environment: Environment::K8s,
            cluster_name: Some("my-cluster".to_string()),
            fleet_id: Some("my-fleet-guid".to_string()),
            pod_name: Some("agent-control-77dcf4b5cd-bkxh8".to_string()),
            node_name: Some("minikube-m02".to_string()),
        };
        let attrs = super::static_resource_attributes(Some(&ctx));

        assert_eq!(
            attribute_value(&attrs, "agent_control.environment").as_deref(),
            Some("kubernetes")
        );
        assert_eq!(
            attribute_value(&attrs, "k8s.cluster.name").as_deref(),
            Some("my-cluster")
        );
        assert_eq!(
            attribute_value(&attrs, FLEET_ID_ATTRIBUTE_KEY).as_deref(),
            Some("my-fleet-guid")
        );
        assert_eq!(
            attribute_value(&attrs, "k8s.pod.name").as_deref(),
            Some("agent-control-77dcf4b5cd-bkxh8")
        );
        assert_eq!(
            attribute_value(&attrs, "k8s.node.name").as_deref(),
            Some("minikube-m02")
        );
    }

    #[test]
    fn test_static_resource_attributes_empty_cluster_and_fleet_are_omitted() {
        // Empty strings (the non-Option default for K8sConfig.cluster_name /
        // OpAMPClientConfig.fleet_id when unset) must not produce empty-valued attributes.
        let ctx = InstanceContext {
            environment: Environment::Linux,
            cluster_name: Some(String::new()),
            fleet_id: Some(String::new()),
            pod_name: Some(String::new()),
            node_name: Some(String::new()),
        };
        let attrs = super::static_resource_attributes(Some(&ctx));

        assert_eq!(attribute_value(&attrs, "k8s.cluster.name"), None);
        assert_eq!(attribute_value(&attrs, FLEET_ID_ATTRIBUTE_KEY), None);
        assert_eq!(attribute_value(&attrs, "k8s.pod.name"), None);
        assert_eq!(attribute_value(&attrs, "k8s.node.name"), None);
    }
}
