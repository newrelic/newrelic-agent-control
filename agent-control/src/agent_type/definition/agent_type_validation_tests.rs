// The scenarios we want to cover here are:
// 1. All agent type definitions are resilient when they are passed values with missing,
// non-required fields (i.e. required values only).
// 2. All passed values conform to the data types expected by the agent type definition.
// 3. All tested agent type definitions are present in the embedded registry.
// 4. All agent type definitions are covered by the test cases (i.e. there are no agent types
// in the registry that are not tested here).

use std::{collections::HashMap, iter, ops::Deref, sync::LazyLock};

use crate::agent_control::run::k8s::{NAMESPACE_AGENTS_VARIABLE_NAME, NAMESPACE_VARIABLE_NAME};
use crate::agent_control::run::on_host::HOST_ID_VARIABLE_NAME;
use crate::{
    agent_control::{agent_id::AgentID, run::Environment},
    agent_type::{
        agent_type_registry::AgentRegistry,
        embedded_registry::EmbeddedRegistry,
        render::{
            persister::config_persister_file::ConfigurationPersisterFile,
            renderer::{Renderer, TemplateRenderer, tests::testing_agent_attributes},
        },
        variable::{Variable, namespace::Namespace},
    },
    sub_agent::effective_agents_assembler::build_agent_type,
    values::yaml_config::YAMLConfig,
};

type CaseDescription = &'static str;
type YamlContents = &'static str;

#[derive(Debug, Default)]
struct AgentTypeValuesTestCase {
    agent_type: &'static str,
    values_k8s: Option<AgentTypeValues>,
    values_onhost: Option<AgentTypeValues>,
}

#[derive(Debug, Default)]
struct AgentTypeValues {
    cases: HashMap<CaseDescription, YamlContents>,
    additional_env: HashMap<String, Variable>,
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
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LICENSE_KEY"),
                    Variable::new_final_string_variable("abcd1234".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_CLUSTER_NAME"),
                    Variable::new_final_string_variable("my-test-cluster".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_STAGING"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LOW_DATA_MODE"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_VERBOSE_LOG"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
            ]),
        }
        .into(),
        values_onhost: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", ""),
                (
                    "check all value types are correct",
                    r#"
                config_agent: "some file contents"
                config_integrations:
                    map_string: "some file contents"
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
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LICENSE_KEY"),
                    Variable::new_final_string_variable("abcd1234".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_CLUSTER_NAME"),
                    Variable::new_final_string_variable("my-test-cluster".to_string()),
                ),
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
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LICENSE_KEY"),
                    Variable::new_final_string_variable("abcd1234".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_CLUSTER_NAME"),
                    Variable::new_final_string_variable("my-test-cluster".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_STAGING"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LOW_DATA_MODE"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_VERBOSE_LOG"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
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
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LICENSE_KEY"),
                    Variable::new_final_string_variable("abcd1234".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_CLUSTER_NAME"),
                    Variable::new_final_string_variable("my-test-cluster".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_STAGING"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LOW_DATA_MODE"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
            ]),
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_OTEL_COLLECTOR: LazyLock<AgentTypeValuesTestCase> =
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
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LICENSE_KEY"),
                    Variable::new_final_string_variable("abcd1234".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_CLUSTER_NAME"),
                    Variable::new_final_string_variable("my-test-cluster".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_STAGING"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LOW_DATA_MODE"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_VERBOSE_LOG"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
            ]),
        }
        .into(),
        values_onhost: AgentTypeValues {
            cases: HashMap::from([
                ("mandatory fields only", ""),
                (
                    "check all value types are correct",
                    r#"
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
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LICENSE_KEY"),
                    Variable::new_final_string_variable("abcd1234".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_CLUSTER_NAME"),
                    Variable::new_final_string_variable("my-test-cluster".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_STAGING"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LOW_DATA_MODE"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_VERBOSE_LOG"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
            ]),
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
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LICENSE_KEY"),
                    Variable::new_final_string_variable("abcd1234".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_CLUSTER_NAME"),
                    Variable::new_final_string_variable("my-test-cluster".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_STAGING"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_VERBOSE_LOG"),
                    Variable::new_final_string_variable("true".to_string()),
                ),
            ]),
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
        &AGENT_TYPE_PIPELINE_CONTROL_GATEWAY,
        &AGENT_TYPE_EBPF,
    ]
    .into_iter()
    .map(Deref::deref)
}

#[test]
fn all_agent_type_definitions_are_present() {
    let registry = EmbeddedRegistry::default();
    for case in get_agent_type_test_cases() {
        assert!(
            registry.get(case.agent_type).is_ok(),
            "Agent type {} not found",
            case.agent_type
        );
    }
}

#[test]
fn all_agent_types_covered_by_tests() {
    let registry = EmbeddedRegistry::default();
    let registry_items = registry.iter_definitions().collect::<Vec<_>>();
    let test_cases = get_agent_type_test_cases().collect::<Vec<_>>();

    assert_eq!(
        registry_items.len(),
        test_cases.len(),
        "Expected the same amount of agent types in the registry and in the test cases"
    );

    let agent_type_names_from_registry = registry_items
        .iter()
        .map(|item| item.agent_type_id.to_string());
    let agent_type_names_from_test_cases = test_cases
        .iter()
        .map(|case| case.agent_type)
        .map(|name| name.to_string())
        .collect::<Vec<_>>();

    agent_type_names_from_registry.for_each(|name| {
        assert!(
            agent_type_names_from_test_cases.contains(&name),
            "Agent type {name} not covered by test cases"
        )
    })
}

#[test]
fn all_agent_type_definitions_are_resilient_k8s() {
    iterate_test_cases(&Environment::K8s);
}

#[test]
fn all_agent_type_definitions_are_resilient_onhost() {
    iterate_test_cases(&Environment::OnHost);
}

fn iterate_test_cases(environment: &Environment) {
    let registry = EmbeddedRegistry::default();
    for case in get_agent_type_test_cases() {
        // Skip cases where values for the environment are not provided
        let Some(values) = (match environment {
            Environment::K8s => &case.values_k8s,
            Environment::OnHost => &case.values_onhost,
        }) else {
            continue;
        };

        let agent_id = AgentID::new("random-agent-id").unwrap();

        // Create the renderer with specifics for the environment
        let renderer: TemplateRenderer<ConfigurationPersisterFile> = match environment {
            Environment::K8s => TemplateRenderer::default().with_agent_control_variables(
                HashMap::from([
                    (
                        NAMESPACE_VARIABLE_NAME.to_string(),
                        Variable::new_final_string_variable("test-namespace".to_string()),
                    ),
                    (
                        NAMESPACE_AGENTS_VARIABLE_NAME.to_string(),
                        Variable::new_final_string_variable("test-namespace-agents".to_string()),
                    ),
                ])
                .into_iter(),
            ),
            Environment::OnHost => {
                TemplateRenderer::default().with_agent_control_variables(iter::once((
                    HOST_ID_VARIABLE_NAME.to_string(),
                    Variable::new_final_string_variable("my-namespace".to_string()),
                )))
            }
        };

        values.cases.iter().for_each(|(scenario, yaml)| {
            let definition = registry.get(case.agent_type).unwrap();
            let agent_type = build_agent_type(definition, environment).unwrap();
            let attributes = testing_agent_attributes(&agent_id);
            let variables = serde_yaml::from_str::<YAMLConfig>(yaml).unwrap();
            let result = renderer.render(
                &agent_id,
                agent_type,
                variables,
                attributes,
                values.additional_env.clone(),
            );

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
