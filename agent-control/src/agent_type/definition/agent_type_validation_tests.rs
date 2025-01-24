// The scenarios we want to cover here are:
// 1. All agent type definitions are resilient when they are passed values with a
//    missing non-required field.
// 2. All agent type definitions are resilient when they are passed values with an additional,
//    unknown variable (cannot know if this variable is optional for the agent type).
// 3. All agent type definitions fail when they are passed values with a missing
//    required field.

use std::{collections::HashMap, iter, ops::Deref, sync::LazyLock};

use crate::{
    agent_control::config::AgentID,
    agent_type::{
        agent_type_registry::AgentRegistry,
        embedded_registry::EmbeddedRegistry,
        environment::Environment,
        renderer::{tests::testing_agent_attributes, Renderer, TemplateRenderer},
        variable::{definition::VariableDefinition, namespace::Namespace},
    },
    sub_agent::{
        effective_agents_assembler::build_agent_type,
        persister::config_persister_file::ConfigurationPersisterFile,
    },
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
    additional_env: HashMap<String, VariableDefinition>,
}

static AGENT_TYPE_APM_DOTNET: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/com.newrelic.apm_dotnet:0.1.0",
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                ("missing non-required fields", r#"version: "some-version""#),
                (
                    "additional unknown variables",
                    r#"random_var: "random-value""#,
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
                ("missing non-required fields", r#"version: "some-version""#),
                (
                    "additional unknown variables",
                    r#"random_var: "random-value""#,
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
                ("missing non-required fields", r#"version: "some-version""#),
                (
                    "additional unknown variables",
                    r#"random_var: "random-value""#,
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
                ("missing non-required fields", r#"version: "some-version""#),
                (
                    "additional unknown variables",
                    r#"random_var: "random-value""#,
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
                ("missing non-required fields", r#"version: "some-version""#),
                (
                    "additional unknown variables",
                    r#"random_var: "random-value""#,
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
                (
                    "missing non-required fields",
                    r#"chart_version: "some-version""#,
                ),
                (
                    "additional unknown variables",
                    r#"
                    chart_version: "some-version"
                    random_var: "random-value"
                    "#,
                ),
            ]),
            additional_env: HashMap::from([
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LICENSE_KEY"),
                    VariableDefinition::new_final_string_variable("abcd1234".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_CLUSTER_NAME"),
                    VariableDefinition::new_final_string_variable("my-test-cluster".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_STAGING"),
                    VariableDefinition::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LOW_DATA_MODE"),
                    VariableDefinition::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_VERBOSE_LOG"),
                    VariableDefinition::new_final_string_variable("true".to_string()),
                ),
            ]),
        }
        .into(),
        values_onhost: AgentTypeValues {
            cases: HashMap::from([
                ("missing non-required fields", ""),
                (
                    "additional unknown variables",
                    r#"random_var: "random-value""#,
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
                (
                    "missing non-required fields",
                    r#"chart_version: "some-version""#,
                ),
                (
                    "additional unknown variables",
                    r#"
                    chart_version: "some-version"
                    random_var: "random-value"
                    "#,
                ),
            ]),
            additional_env: HashMap::from([
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LICENSE_KEY"),
                    VariableDefinition::new_final_string_variable("abcd1234".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_CLUSTER_NAME"),
                    VariableDefinition::new_final_string_variable("my-test-cluster".to_string()),
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
                (
                    "missing non-required fields",
                    r#"chart_version: "some-version""#,
                ),
                (
                    "additional unknown variables",
                    r#"
                    chart_version: "some-version"
                    random_var: "random-value"
                    "#,
                ),
            ]),
            additional_env: HashMap::from([
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LICENSE_KEY"),
                    VariableDefinition::new_final_string_variable("abcd1234".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_CLUSTER_NAME"),
                    VariableDefinition::new_final_string_variable("my-test-cluster".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_STAGING"),
                    VariableDefinition::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LOW_DATA_MODE"),
                    VariableDefinition::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_VERBOSE_LOG"),
                    VariableDefinition::new_final_string_variable("true".to_string()),
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
                (
                    "missing non-required fields",
                    r#"chart_version: "some-version""#,
                ),
                (
                    "additional unknown variables",
                    r#"
                    chart_version: "some-version"
                    random_var: "random-value"
                    "#,
                ),
            ]),
            additional_env: HashMap::from([
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LICENSE_KEY"),
                    VariableDefinition::new_final_string_variable("abcd1234".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_CLUSTER_NAME"),
                    VariableDefinition::new_final_string_variable("my-test-cluster".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_STAGING"),
                    VariableDefinition::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LOW_DATA_MODE"),
                    VariableDefinition::new_final_string_variable("true".to_string()),
                ),
            ]),
        }
        .into(),
        ..Default::default()
    });

static AGENT_TYPE_OTEL_COLLECTOR: LazyLock<AgentTypeValuesTestCase> =
    LazyLock::new(|| AgentTypeValuesTestCase {
        agent_type: "newrelic/io.opentelemetry.collector:0.2.0", // FIXME after PR #1010 is merged
        values_k8s: AgentTypeValues {
            cases: HashMap::from([
                (
                    "missing non-required fields",
                    r#"chart_version: "some-version""#,
                ),
                (
                    "additional unknown variables",
                    r#"
                    chart_version: "some-version"
                    random_var: "random-value"
                    "#,
                ),
            ]),
            additional_env: HashMap::from([
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LICENSE_KEY"),
                    VariableDefinition::new_final_string_variable("abcd1234".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_CLUSTER_NAME"),
                    VariableDefinition::new_final_string_variable("my-test-cluster".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_STAGING"),
                    VariableDefinition::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LOW_DATA_MODE"),
                    VariableDefinition::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_VERBOSE_LOG"),
                    VariableDefinition::new_final_string_variable("true".to_string()),
                ),
            ]),
        }
        .into(),
        values_onhost: AgentTypeValues {
            cases: HashMap::from([
                ("missing non-required fields", ""),
                (
                    "additional unknown variables",
                    r#"random_var: "random-value""#,
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
                (
                    "missing non-required fields",
                    r#"chart_version: "some-version""#,
                ),
                (
                    "additional unknown variables",
                    r#"
                    chart_version: "some-version"
                    random_var: "random-value"
                    "#,
                ),
            ]),
            additional_env: HashMap::from([
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LICENSE_KEY"),
                    VariableDefinition::new_final_string_variable("abcd1234".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_CLUSTER_NAME"),
                    VariableDefinition::new_final_string_variable("my-test-cluster".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_STAGING"),
                    VariableDefinition::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_LOW_DATA_MODE"),
                    VariableDefinition::new_final_string_variable("true".to_string()),
                ),
                (
                    Namespace::EnvironmentVariable.namespaced_name("NR_VERBOSE_LOG"),
                    VariableDefinition::new_final_string_variable("true".to_string()),
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
            Environment::K8s => TemplateRenderer::default(),
            Environment::OnHost => {
                TemplateRenderer::default().with_agent_control_variables(iter::once((
                    "host_id".to_string(),
                    VariableDefinition::new_final_string_variable("host-id".to_string()),
                )))
            }
        };

        for (scenario, yaml) in values.cases.iter() {
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
            );
        }
    }
}
