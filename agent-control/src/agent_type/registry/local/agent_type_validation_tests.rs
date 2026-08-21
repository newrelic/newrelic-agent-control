// The scenarios we want to cover here are:
// 1. All agent type definitions are resilient when they are passed values with missing,
// non-required fields (i.e. required values only).
// 2. All passed values conform to the data types expected by the agent type definition.
// 3. All tested agent type definitions are present in the embedded registry.
// 4. All agent type definitions are covered by the test cases (i.e. there are no agent types
// in the registry that are not tested here).

use super::LocalRegistry;
use crate::agent_control::run::k8s::{NAMESPACE_AGENTS_VARIABLE_NAME, NAMESPACE_VARIABLE_NAME};
use crate::agent_control::run::on_host::HOST_ID_VARIABLE_NAME;
use crate::agent_type::variable::constraints::VariableConstraints;
use crate::agent_type::variable::namespace::{Namespace, VariableName};
use crate::environment::Environment;
use crate::sub_agent::agent_renderer::{
    AgentRenderer, Renderer, tests::env_secrets_registry_for_testing,
};
use crate::sub_agent::identity::AgentIdentity;
use crate::{
    agent_control::agent_id::AgentID,
    agent_type::{agent_type_id::AgentTypeID, variable::Variable},
    values::yaml_config::YAMLConfig,
};
use std::collections::HashSet;
use std::{
    collections::HashMap,
    ops::Deref,
    sync::{Arc, LazyLock},
};

type CaseDescription = &'static str;
type YamlContents = &'static str;

#[derive(Debug, Default)]
struct AgentTypeValuesTestCase {
    agent_type: &'static str,
    values_k8s: Option<AgentTypeValues>,
    values_windows: Option<AgentTypeValues>,
    values_linux: Option<AgentTypeValues>,
}

#[derive(Debug, Default)]
struct AgentTypeValues {
    cases: HashMap<CaseDescription, YamlContents>,
    additional_env: HashMap<&'static str, &'static str>,
}

static AGENT_TYPE_APM_DOTNET: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.apm_dotnet:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                version: "some-string"
                podLabelSelector:
                    yaml: object
                namespaceLabelSelector:
                    yaml: object
                env:
                    - SOME_VAR: "some-value"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_APM_JAVA: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.apm_java:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                version: "some-string"
                podLabelSelector:
                    yaml: object
                namespaceLabelSelector:
                    yaml: object
                env:
                    - SOME_VAR: "some-value"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_APM_NODE: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.apm_node:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                version: "some-string"
                podLabelSelector:
                    yaml: object
                namespaceLabelSelector:
                    yaml: object
                env:
                    - SOME_VAR: "some-value"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_APM_PYTHON: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.apm_python:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                version: "some-string"
                podLabelSelector:
                    yaml: object
                namespaceLabelSelector:
                    yaml: object
                env:
                    - SOME_VAR: "some-value"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_APM_RUBY: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.apm_ruby:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                version: "some-string"
                podLabelSelector:
                    yaml: object
                namespaceLabelSelector:
                    yaml: object
                env:
                    - SOME_VAR: "some-value"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_INFRASTRUCTURE: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.infrastructure:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"chart_version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                chart_version: "some-version"
                chart_values.newrelic-infrastructure:
                    yaml: object
                chart_values.nri-metadata-injection:
                    yaml: object
                chart_values.kube-state-metrics:
                    yaml: object
                chart_values.nri-kube-events:
                    yaml: object
                chart_values.global:
                    yaml: object
                "#,
                ),
            ]),
            additional_env: HashMap::from([
                ("NR_LICENSE_KEY", "abcd1234"),
                ("NR_CLUSTER_NAME", "my-test-cluster"),
                ("NR_STAGING", "true"),
                ("NR_LOW_DATA_MODE", "true"),
                ("NR_VERBOSE_LOG", "true"),
            ]),
        }
        .into(),
        values_linux: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                version: "some-version"
                config_agent: "some file contents"
                config_logging:
                    map_string: "some file contents"
                backoff_delay: "10s"
                enable_file_logging: true
                health_port: 12345
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        values_windows: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                version: "some-version"
                config_agent: "some file contents"
                config_logging:
                    map_string: "some file contents"
                backoff_delay: "10s"
                enable_file_logging: true
                health_port: 12345
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
    });

static AGENT_TYPE_K8S_AGENT_OPERATOR: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.k8s_agent_operator:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"chart_version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                chart_version: "some-version"
                chart_values.k8s-agents-operator:
                    yaml: object
                chart_values.global:
                    yaml: object
                "#,
                ),
            ]),
            additional_env: HashMap::from([
                ("NR_LICENSE_KEY", "abcd1234"),
                ("NR_CLUSTER_NAME", "my-test-cluster"),
            ]),
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_PROMETHEUS: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.prometheus:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"chart_version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                chart_version: "some-version"
                chart_values.newrelic-prometheus-agent:
                    yaml: object
                chart_values.global:
                    yaml: object
                "#,
                ),
            ]),
            additional_env: HashMap::from([
                ("NR_LICENSE_KEY", "abcd1234"),
                ("NR_CLUSTER_NAME", "my-test-cluster"),
                ("NR_STAGING", "true"),
                ("NR_LOW_DATA_MODE", "true"),
                ("NR_VERBOSE_LOG", "true"),
            ]),
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_FLUENTBIT: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/io.fluentbit:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"chart_version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                chart_version: "some-version"
                chart_values.newrelic-logging:
                    yaml: object
                chart_values.global:
                    yaml: object
                "#,
                ),
            ]),
            additional_env: HashMap::from([
                ("NR_LICENSE_KEY", "abcd1234"),
                ("NR_CLUSTER_NAME", "my-test-cluster"),
                ("NR_STAGING", "true"),
                ("NR_LOW_DATA_MODE", "true"),
            ]),
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_OTEL_COLLECTOR: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.opentelemetry.collector:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"chart_version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                chart_version: "some-version"
                chart_values.nr-k8s-otel-collector:
                    yaml: object
                chart_values.global:
                    yaml: object
                "#,
                ),
            ]),
            additional_env: HashMap::from([
                ("NR_LICENSE_KEY", "abcd1234"),
                ("NR_CLUSTER_NAME", "my-test-cluster"),
                ("NR_STAGING", "true"),
                ("NR_LOW_DATA_MODE", "true"),
                ("NR_VERBOSE_LOG", "true"),
            ]),
        }
        .into(),
        values_linux: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                version: "some-version"
                config: "some file contents"
                backoff_delay: "10s"
                health_check.path: "/health"
                health_check.port: 12345
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        values_windows: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                version: "some-version"
                config: "some file contents"
                backoff_delay: "10s"
                health_check.path: "/health"
                health_check.port: 12345
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
    });

static AGENT_TYPE_OTEL_COLLECTOR_OLD: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/io.opentelemetry.collector:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"chart_version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                chart_version: "some-version"
                chart_values.nr-k8s-otel-collector:
                    yaml: object
                chart_values.global:
                    yaml: object
                "#,
                ),
            ]),
            additional_env: HashMap::from([
                ("NR_LICENSE_KEY", "abcd1234"),
                ("NR_CLUSTER_NAME", "my-test-cluster"),
                ("NR_STAGING", "true"),
                ("NR_LOW_DATA_MODE", "true"),
                ("NR_VERBOSE_LOG", "true"),
            ]),
        }
        .into(),
        values_linux: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", ""),
                (
                    "check all value types are correct",
                    r#"
                version: "some-version"
                config: "some file contents"
                backoff_delay: "10s"
                health_check.path: "/health"
                health_check.port: 12345
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_PIPELINE_CONTROL_GATEWAY: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.pipeline_control_gateway:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"chart_version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                chart_version: "some-version"
                chart_values.gateway:
                    yaml: object
                chart_values.global:
                    yaml: object
                "#,
                ),
            ]),
            additional_env: HashMap::from([
                ("NR_LICENSE_KEY", "abcd1234"),
                ("NR_CLUSTER_NAME", "my-test-cluster"),
                ("NR_STAGING", "true"),
                ("NR_LOW_DATA_MODE", "true"),
                ("NR_VERBOSE_LOG", "true"),
            ]),
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_PIPELINE_CONTROL_GATEWAY_CONFIG: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.pipeline_control_gateway_config:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                (
                    "mandatory fields only",
                    r#"
                config:
                    receivers:
                        otlp: {}
                "#,
                ),
                (
                    "check all value types are correct",
                    r#"
                config:
                    receivers:
                        otlp: {}
                    exporters:
                        otlp:
                            endpoint: "otlp.nr-data.net:4317"
                    service:
                        pipelines:
                            traces:
                                receivers: [otlp]
                                exporters: [otlp]
                config_map_name: "my-otel-config"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_PIPELINE_CONTROL_GATEWAY_CONFIG_MODE: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.pipeline_control_gateway_config_mode:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                (
                    "mandatory fields only",
                    r#"
                config:
                    receivers:
                        otlp: {}
                "#,
                ),
                (
                    "check all value types are correct",
                    r#"
                config:
                    receivers:
                        otlp: {}
                    exporters:
                        otlp:
                            endpoint: "otlp.nr-data.net:4317"
                    service:
                        pipelines:
                            traces:
                                receivers: [otlp]
                                exporters: [otlp]
                config_map_name: "my-otel-config"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_EBPF: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.ebpf:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", r#"chart_version: "some-version""#),
                (
                    "check all value types are correct",
                    r#"
                chart_version: "some-version"
                chart_values.nr-ebpf-agent:
                    yaml: object
                chart_values.global:
                    yaml: object
                "#,
                ),
            ]),
            additional_env: HashMap::from([
                ("NR_LICENSE_KEY", "abcd1234"),
                ("NR_CLUSTER_NAME", "my-test-cluster"),
                ("NR_STAGING", "true"),
                ("NR_VERBOSE_LOG", "true"),
            ]),
        }
        .into(),
        values_linux: AgentTypeValues {
            // TODO test on linux needs to be added
            cases: HashMap::new(),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_REDIS: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.infrastructure.nri_redis:0.1.0",
        values_linux: AgentTypeValues {
            cases: HashMap::from([
                (
                    "mandatory fields only",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                "#,
                ),
                (
                    "check all value types are correct",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                oci.repository: "newrelic/nri-redis"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        values_windows: AgentTypeValues {
            cases: HashMap::from([
                (
                    "mandatory fields only",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                "#,
                ),
                (
                    "check all value types are correct",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                oci.repository: "newrelic/nri-redis"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_NGINX: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.infrastructure.nri_nginx:0.1.0",
        values_linux: AgentTypeValues {
            cases: HashMap::from([
                (
                    "mandatory fields only",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                "#,
                ),
                (
                    "check all value types are correct",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                oci.repository: "newrelic/nri-nginx"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        values_windows: AgentTypeValues {
            cases: HashMap::from([
                (
                    "mandatory fields only",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                "#,
                ),
                (
                    "check all value types are correct",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                oci.repository: "newrelic/nri-nginx"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_APACHE: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.infrastructure.nri_apache:0.1.0",
        values_linux: AgentTypeValues {
            cases: HashMap::from([
                (
                    "mandatory fields only",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                "#,
                ),
                (
                    "check all value types are correct",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                oci.repository: "newrelic/nri-apache"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        values_windows: AgentTypeValues {
            cases: HashMap::from([
                (
                    "mandatory fields only",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                "#,
                ),
                (
                    "check all value types are correct",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                oci.repository: "newrelic/nri-apache"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_POSTGRESQL: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.infrastructure.nri_postgresql:0.1.0",
        values_linux: AgentTypeValues {
            cases: HashMap::from([
                (
                    "mandatory fields only",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                "#,
                ),
                (
                    "check all value types are correct",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                oci.repository: "newrelic/nri-postgresql"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        values_windows: AgentTypeValues {
            cases: HashMap::from([
                (
                    "mandatory fields only",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                "#,
                ),
                (
                    "check all value types are correct",
                    r#"
                config: "integrations: []"
                version: "v1.15.2"
                oci.repository: "newrelic/nri-postgresql"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_MYSQL: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.infrastructure.nri_mysql:0.1.0",
        values_linux: AgentTypeValues {
            cases: HashMap::from([
                (
                    "mandatory fields only",
                    r#"
                config: "integrations: []"
                version: "v1.23.2"
                "#,
                ),
                (
                    "check all value types are correct",
                    r#"
                config: "integrations: []"
                version: "v1.23.2"
                oci.repository: "newrelic/nri-mysql"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        values_windows: AgentTypeValues {
            cases: HashMap::from([
                (
                    "mandatory fields only",
                    r#"
                config: "integrations: []"
                version: "v1.23.2"
                "#,
                ),
                (
                    "check all value types are correct",
                    r#"
                config: "integrations: []"
                version: "v1.23.2"
                oci.repository: "newrelic/nri-mysql"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_MEMCACHED: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.infrastructure.nri_memcached:0.1.0",
        values_linux: AgentTypeValues {
            cases: HashMap::from([
                (
                    "mandatory fields only",
                    r#"
                config: "integrations: []"
                version: "2.9.3"
                "#,
                ),
                (
                    "check all value types are correct",
                    r#"
                config: "integrations: []"
                version: "2.9.3"
                oci.repository: "newrelic/nri-memcached"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        values_windows: AgentTypeValues {
            cases: HashMap::from([
                (
                    "mandatory fields only",
                    r#"
                config: "integrations: []"
                version: "2.9.3"
                "#,
                ),
                (
                    "check all value types are correct",
                    r#"
                config: "integrations: []"
                version: "2.9.3"
                oci.repository: "newrelic/nri-memcached"
                "#,
                ),
            ]),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    });

fn get_agent_type_test_cases() -> impl Iterator<Item = &'static AgentTypeValuesTestCase> {
    [
        &AGENT_TYPE_APM_DOTNET,
        &AGENT_TYPE_APM_JAVA,
        &AGENT_TYPE_APM_NODE,
        &AGENT_TYPE_APM_PYTHON,
        &AGENT_TYPE_APM_RUBY,
        &AGENT_TYPE_INFRASTRUCTURE,
        &AGENT_TYPE_K8S_AGENT_OPERATOR,
        &AGENT_TYPE_PROMETHEUS,
        &AGENT_TYPE_FLUENTBIT,
        &AGENT_TYPE_OTEL_COLLECTOR,
        &AGENT_TYPE_OTEL_COLLECTOR_OLD,
        &AGENT_TYPE_PIPELINE_CONTROL_GATEWAY,
        &AGENT_TYPE_PIPELINE_CONTROL_GATEWAY_CONFIG_MODE,
        &AGENT_TYPE_PIPELINE_CONTROL_GATEWAY_CONFIG,
        &AGENT_TYPE_EBPF,
        &AGENT_TYPE_REDIS,
        &AGENT_TYPE_NGINX,
        &AGENT_TYPE_APACHE,
        &AGENT_TYPE_POSTGRESQL,
        &AGENT_TYPE_MYSQL,
        &AGENT_TYPE_MEMCACHED,
    ]
    .into_iter()
    .map(Deref::deref)
}

#[test]
fn all_agent_type_definitions_are_present() {
    for env in [Environment::K8s, Environment::Linux, Environment::Windows] {
        let mut definitions: HashSet<AgentTypeID> = LocalRegistry::embedded_only(env)
            .iter_definitions()
            .map(|d| d.agent_type_id().clone())
            .collect();
        for case in get_agent_type_test_cases() {
            if match env {
                Environment::K8s => case.values_k8s.is_none(),
                Environment::Linux => case.values_linux.is_none(),
                Environment::Windows => case.values_windows.is_none(),
            } {
                continue;
            }
            let id = AgentTypeID::try_from(case.agent_type).unwrap();
            assert!(
                definitions.take(&id).is_some(),
                "Agent type {} not found in {env}",
                case.agent_type
            );
        }
        assert!(
            definitions.is_empty(),
            "Following agent types in {env} don't have tests: {:?}",
            definitions
        )
    }
}

#[test]
fn all_agent_type_definitions_are_resilient_k8s() {
    iterate_test_cases(Environment::K8s);
}

#[test]
fn all_agent_type_definitions_are_resilient_linux() {
    iterate_test_cases(Environment::Linux);
}

#[test]
fn all_agent_type_definitions_are_resilient_windows() {
    iterate_test_cases(Environment::Windows);
}

fn iterate_test_cases(environment: Environment) {
    let registry = Arc::new(LocalRegistry::embedded_only(environment));

    // Agent-control variables with specifics for the environment
    let ac_variables: HashMap<VariableName, Variable> = match environment {
        Environment::K8s => HashMap::from([
            (
                VariableName::new(Namespace::AgentControl, NAMESPACE_VARIABLE_NAME),
                Variable::new_final_string_variable("test-namespace".to_string()),
            ),
            (
                VariableName::new(Namespace::AgentControl, NAMESPACE_AGENTS_VARIABLE_NAME),
                Variable::new_final_string_variable("test-namespace-agents".to_string()),
            ),
        ]),
        Environment::Linux | Environment::Windows => HashMap::from([(
            VariableName::new(Namespace::AgentControl, HOST_ID_VARIABLE_NAME),
            Variable::new_final_string_variable("my-namespace".to_string()),
        )]),
    };

    #[cfg(windows)]
    let remote_dir = std::path::PathBuf::from("C:\\");
    #[cfg(not(windows))]
    let remote_dir = std::path::PathBuf::from("/");

    for case in get_agent_type_test_cases() {
        // Skip cases where values for the environment are not provided
        let Some(values) = (match environment {
            Environment::K8s => &case.values_k8s,
            Environment::Linux => &case.values_linux,
            Environment::Windows => &case.values_windows,
        }) else {
            continue;
        };

        let agent_identity = AgentIdentity::from((
            AgentID::try_from("random-agent-id").unwrap(),
            AgentTypeID::try_from(case.agent_type).unwrap(),
        ));

        let renderer = AgentRenderer::new(
            registry.clone(),
            ac_variables.clone(),
            VariableConstraints::default(),
            env_secrets_registry_for_testing(values.additional_env.clone()),
            &remote_dir,
        );

        values.cases.iter().for_each(|(scenario, yaml)| {
            let yaml_config = serde_saphyr::from_str::<YAMLConfig>(yaml).unwrap();

            let result = renderer.render_agent(&agent_identity, yaml_config);

            assert!(
                result.is_ok(),
                "{:?} scenario: {} -- Failed to fill variables for {}: {:#?}",
                environment,
                scenario,
                case.agent_type,
                result
            )
        });
    }
}
