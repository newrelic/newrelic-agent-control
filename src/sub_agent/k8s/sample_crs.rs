/// This module includes some sample hard-coded Flux's CRs manifests to be used in early development stages.

/// It defines a Flux's HelmRepository object corresponding to the OpenTelemetry helm charts repository.
pub const OTEL_HELM_REPOSITORY_CR: &str = r#"
apiVersion: source.toolkit.fluxcd.io/v1beta2
kind: HelmRepository
metadata:
  name: open-telemetry
  namespace: default
spec:
  interval: 1m
  url: https://open-telemetry.github.io/opentelemetry-helm-charts
"#;

/// It defines a Flux's HelmRelease object with the OpenTelemetry collector configured to scrape self metrics through
/// the prometheus receiver and send them to New Relic.
/// It requires the `newrelic-secrets` secret in the same namespace of the repository cluster.
///
/// Secret manifest example:
/// ```yaml
/// apiVersion: v1
/// kind: Secret
/// metadata:
///   name: newrelic-secrets
/// stringData:
///   NEWRELIC_LICENSE_KEY: <your-license-key>
/// ````
pub const OTELCOL_HELM_RELEASE_CR: &str = r#"
apiVersion: helm.toolkit.fluxcd.io/v2beta1
kind: HelmRelease
metadata:
  name: otel-collector
  namespace: default
spec:
  interval: 1h0m0s
  chart:
    spec:
      chart: opentelemetry-collector
      version: 0.73.1
      sourceRef:
        kind: HelmRepository
        name: open-telemetry
        namespace: default
  releaseName: otel-collector
  targetNamespace: default
  values:
    mode: deployment
    extraEnvs:
      - name: NEWRELIC_LICENSE_KEY
        valueFrom:
          secretKeyRef: # This assumes the existence of 'newrelic-secrets' secret in the same namespace.
            name: newrelic-secrets
            key: NEWRELIC_LICENSE_KEY
    config:
      receivers:
        prometheus:
          config:
            scrape_configs:
              - job_name: otel-collector-self-metrics
                scrape_interval: 10s
                static_configs:
                  - targets: [localhost:8888]

      exporters:
        otlp/newrelic_coreint:
          endpoint: "https://staging-otlp.nr-data.net:443"
          headers:
            api-key: "${env:NEWRELIC_LICENSE_KEY}"

      extensions:
        # The health_check extension is mandatory for this chart.
        # Without the health_check extension the collector will fail the readiness and liveliness probes.
        # The health_check extension can be modified, but should never be removed.
        health_check: {}
        memory_ballast: {}

      processors:
        batch: {}
        # If set to null, will be overridden with values based on k8s resource limits
        memory_limiter: null
        attributes/cluster_name:
          actions:
            - key: cluster_name # This attribute is used internally and do not follow any convention
              value: super-agent-dev
              action: insert

      service:
        telemetry:
          metrics:
            address: 0.0.0.0:8888
          logs:
            level: "info"
        extensions:
          - health_check
          - memory_ballast
        pipelines:
          metrics:
            exporters:
              - otlp/newrelic_coreint
            processors:
              - memory_limiter
              - batch
              - attributes/cluster_name
            # each specific canary is supposed to set the corresponding receivers

    image:
      # If you want to use the core image `otel/opentelemetry-collector`, you also need to change `command.name` value to `otelcol`.
      repository: otel/opentelemetry-collector-contrib
      pullPolicy: IfNotPresent
      # NRDOT example:
      # repository: newrelic/nr-otel-collector
      # pullPolicy: IfNotPresent
      # Overrides the image tag whose default is the chart appVersion.
      # tag: "0.0.1-rc"

    # OpenTelemetry Collector executable
    command:
      name: otelcol-contrib
      # NRDOT example:
      # name: nr-otel-collector
      # disable prometheus.Normalize name to avoid altering prometheus metrics names
      # check <https://github.com/open-telemetry/opentelemetry-collector-contrib/issues/21743> for additional context.
      # extraArgs: ["--feature-gates=-pkg.translator.prometheus.NormalizeName"]
"#;

#[cfg(test)]
mod test {
    use super::*;
    use serde_yaml;

    #[test]
    fn check_examples_are_valid_yaml() {
        serde_yaml::from_str::<serde_yaml::Value>(OTEL_HELM_REPOSITORY_CR).unwrap();
        serde_yaml::from_str::<serde_yaml::Value>(OTELCOL_HELM_RELEASE_CR).unwrap();
    }
}
