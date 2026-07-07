# Untype Agent-Type Variables — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove `pub enum VariableTypeDefinition` and drop the `type:` field from agent-type variables. Introduce a `| toYAML` pipe (Helm-inspired but simpler) that substitutes a variable's raw YAML value in-place. Default rendering becomes "always string."

**Architecture:** Collapse the typed variant enum (`String | Bool | Number | Yaml | MapStringYaml`) into a single untyped `Variable` backed by `serde_json::Value`. The renderer inspects placeholder pipes: if `| toYAML` is present on a lone placeholder, substitute the raw YAML value; otherwise stringify. Migration happens in phases: introduce the pipe → migrate callsites → rewrite the model → clean up.

**Tech Stack:** Rust, `serde_json`, `serde_saphyr`, `regex`, `rstest`, `assert_matches`.

**Reference spec:** `docs/superpowers/specs/2026-07-06-untype-variables-design.md`

---

## File Structure

**Delete after Task 5:**
- `agent-control/src/agent_type/variable/variable_type.rs`
- `agent-control/src/agent_type/trivial_value.rs`
- `agent-control/src/agent_type/variable/fields.rs`

**Rewrite in Task 5:**
- `agent-control/src/agent_type/variable.rs` — new `Variable` / `VariableDefinition` shapes.
- `agent-control/src/agent_type/variable/variants.rs` — specialize to `serde_json::Value`.
- `agent-control/src/agent_type/templates.rs` — drop type-based branching.

**Touched throughout:**
- `agent-control/src/agent_type/templates_function.rs` — new `ToYaml` variant (Task 1).
- `agent-control/src/agent_type/definition.rs` — fill logic + tests (Task 5).
- `agent-control/src/agent_type/render.rs` — test constants (Task 5).
- `agent-control/src/agent_type/registry/` — no logic changes expected; touch only if callers of the old `VariableType::*` need adaptation.
- `agent-control/src/agent_type/variable/tree.rs`, `namespace.rs`, `constraints.rs` — verify still compiling.
- `agent-control/agent-type-registry/newrelic/*.yaml` (21 files) — Task 3 (add `| toYAML`), Task 6 (strip `type:`).
- `agent-control/tests/**/*.yml`, `test/**/*.yml`, `agent-control/tests/**/*.rs` — Task 3, Task 5, Task 6.

Each Rust file's responsibility stays what it is today. The one restructuring choice: `fields.rs` disappears (its content collapses into `variable.rs`), because it exists solely to parameterize by the deleted type enum.

---

### Task 1: Add the `ToYaml` pipe function (identity apply)

**Files:**
- Modify: `agent-control/src/agent_type/templates_function.rs`

The pipe parser must accept `toYAML` before we can migrate callsites in Task 3. In this task the pipe is identity when applied to a string; the tree-substitution semantics land in Task 2.

- [ ] **Step 1: Add failing tests for `toYAML` parsing and identity apply**

Append to the `tests` module at the bottom of `agent-control/src/agent_type/templates_function.rs`:

```rust
#[rstest]
#[case::plain("|toYAML")]
#[case::spaces("| toYAML")]
#[case::case_insensitive_lower("|toyaml")]
#[case::case_insensitive_upper("|TOYAML")]
fn test_parse_toyaml(#[case] functions_str: &str) {
    let functions = SupportedFunction::parse_function_list(functions_str).unwrap();
    assert_eq!(functions.len(), 1);
    assert!(matches!(functions[0], SupportedFunction::ToYaml(_)));
}

#[test]
fn test_toyaml_apply_is_identity() {
    let functions = SupportedFunction::parse_function_list("|toYAML").unwrap();
    let out = functions
        .iter()
        .try_fold("hello".to_string(), |acc, f| f.apply(acc))
        .unwrap();
    assert_eq!(out, "hello");
}

#[test]
fn test_parse_mixed_indent_and_toyaml() {
    let functions = SupportedFunction::parse_function_list("|toYAML|indent 2").unwrap();
    assert_eq!(functions.len(), 2);
    assert!(matches!(functions[0], SupportedFunction::ToYaml(_)));
    assert!(matches!(functions[1], SupportedFunction::Indent(_)));
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p agent-control --lib agent_type::templates_function::tests::test_parse_toyaml`
Expected: FAIL — `ToYaml` variant does not exist.

- [ ] **Step 3: Add the `ToYaml` variant and dispatch**

Edit `agent-control/src/agent_type/templates_function.rs`. Add after the `Indent` struct/impl block, before `SupportedFunction`:

```rust
const TOYAML_FUNCTION_NAME: &str = "toyaml";

/// Marker pipe indicating the variable's raw YAML value should be substituted
/// in-place. Behavior is implemented in the renderer; in a string-only pipeline
/// this is an identity transform.
#[derive(Debug, PartialEq)]
pub struct ToYaml;

impl Function for ToYaml {
    fn apply(&self, value: String) -> Result<String, FunctionError> {
        Ok(value)
    }

    fn parse(value: &str) -> Result<Self, FunctionError> {
        let trimmed = value.trim().to_ascii_lowercase();
        if trimmed != TOYAML_FUNCTION_NAME {
            return Err(FunctionError::UnknownFunctionName(value.to_string()));
        }
        Ok(ToYaml)
    }
}
```

Then update `SupportedFunction`:

```rust
#[derive(Debug, PartialEq)]
pub enum SupportedFunction {
    /// The `indent` function.
    Indent(Indent),
    /// The `toYAML` marker pipe.
    ToYaml(ToYaml),
}
```

Update `SupportedFunction`'s `Function` impl:

```rust
impl Function for SupportedFunction {
    fn apply(&self, value: String) -> Result<String, FunctionError> {
        match self {
            SupportedFunction::Indent(indent) => indent.apply(value),
            SupportedFunction::ToYaml(f) => f.apply(value),
        }
    }
    fn parse(value: &str) -> Result<Self, FunctionError> {
        match ToYaml::parse(value) {
            Ok(f) => return Ok(Self::ToYaml(f)),
            Err(FunctionError::UnknownFunctionName(_)) => {}
            Err(e) => return Err(e),
        }
        Ok(Self::Indent(Indent::parse(value)?))
    }
}
```

- [ ] **Step 4: Run the new tests**

Run: `cargo test -p agent-control --lib agent_type::templates_function::tests`
Expected: PASS — all existing indent tests plus the four new tests.

- [ ] **Step 5: Run the full workspace tests**

Run: `cargo test -p agent-control --lib`
Expected: PASS — nothing else depends on `SupportedFunction`'s shape yet.

- [ ] **Step 6: Commit**

```bash
git add agent-control/src/agent_type/templates_function.rs
git commit -m "$(cat <<'EOF'
feat(agent-type): add toYAML pipe parser

Introduces a `toYAML` variant to `SupportedFunction`. The pipe is an
identity transform when applied to a string; its purpose is to be
detected by the renderer for in-place YAML tree substitution (added in
a follow-up commit).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Wire `| toYAML` into the renderer alongside legacy type-based branching

**Files:**
- Modify: `agent-control/src/agent_type/templates.rs`

Fix `template_yaml_value_string` so a lone placeholder's pipe list is parsed. If the pipe list contains `ToYaml`, substitute the raw yaml value. Otherwise fall through to the existing type-based logic. This preserves backward compatibility during Task 3's callsite migration.

- [ ] **Step 1: Add failing test for `| toYAML` in a lone placeholder**

Append to `tests` module in `agent-control/src/agent_type/templates.rs`:

```rust
#[test]
fn test_toyaml_pipe_substitutes_yaml_tree_when_alone() {
    let variables = Variables::from([(
        "nr-var:yaml.var".to_string(),
        Variable::new(
            String::default(),
            true,
            None,
            Some(serde_json::Value::Object(serde_json::Map::from_iter([(
                "key".into(),
                "value".into(),
            )]))),
        ),
    )]);
    let input: serde_json::Value = serde_json::Value::String("${nr-var:yaml.var | toYAML}".into());
    let output = input.template_with(&variables).unwrap();
    assert_eq!(
        output,
        serde_json::Value::Object(serde_json::Map::from_iter([(
            "key".into(),
            "value".into()
        )]))
    );
}

#[test]
fn test_toyaml_pipe_errors_when_combined_with_indent() {
    let variables = Variables::from([(
        "nr-var:yaml.var".to_string(),
        Variable::new(
            String::default(),
            true,
            None,
            Some(serde_json::Value::Object(serde_json::Map::from_iter([(
                "key".into(),
                "value".into(),
            )]))),
        ),
    )]);
    let input: serde_json::Value =
        serde_json::Value::String("${nr-var:yaml.var | toYAML | indent 2}".into());
    let err = input.template_with(&variables).unwrap_err();
    assert_matches!(err, AgentTypeError::RenderingTemplate(_));
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p agent-control --lib agent_type::templates::tests::test_toyaml_pipe`
Expected: FAIL — the lone branch does not parse pipes yet.

- [ ] **Step 3: Rewrite `template_yaml_value_string` to parse pipes**

Replace the body of `template_yaml_value_string` in `agent-control/src/agent_type/templates.rs`:

```rust
fn template_yaml_value_string(
    s: String,
    variables: &Variables,
) -> Result<serde_json::Value, AgentTypeError> {
    // When there is more content than a variable template, template as a regular string.
    if !only_template_var_re().is_match(s.as_str()) {
        let templated = template_string(s, variables)?;
        return Ok(serde_json::Value::String(templated));
    }
    // Otherwise, parse the lone placeholder and dispatch.
    let captures = template_re()
        .captures(s.as_str())
        .expect("only_template_var_re matched; template_re must too");
    let var_ref = captures.get(1).unwrap().as_str();
    let pipe_str = captures.get(2).map(|m| m.as_str()).unwrap_or("");

    let functions = SupportedFunction::parse_function_list(pipe_str)
        .map_err(|e| AgentTypeError::RenderingTemplate(e.to_string()))?;

    let var_spec = normalized_var(var_ref, variables)?;
    let var_value = var_spec
        .get_final_value()
        .ok_or(AgentTypeError::MissingValue(var_ref.to_string()))?;

    // `toYAML` pipe short-circuits to raw YAML substitution.
    let has_toyaml = functions
        .iter()
        .any(|f| matches!(f, SupportedFunction::ToYaml(_)));
    if has_toyaml {
        if functions.len() > 1 {
            return Err(AgentTypeError::RenderingTemplate(
                "the `toYAML` pipe cannot be combined with other pipes".to_string(),
            ));
        }
        return var_value
            .to_yaml_value()
            .ok_or(AgentTypeError::UnexpectedValueForKey(
                var_ref.to_string(),
                var_value.to_string(),
            ));
    }

    // Legacy type-based branching (removed in Task 5).
    match var_spec.kind() {
        VariableType::Yaml(_) => {
            var_value
                .to_yaml_value()
                .ok_or(AgentTypeError::UnexpectedValueForKey(
                    var_ref.to_string(),
                    var_value.to_string(),
                ))
        }
        VariableType::Bool(_) | VariableType::Number(_) => {
            serde_saphyr::from_str(var_value.to_string().as_str())
                .map_err(AgentTypeError::Serialization)
        }
        _ => {
            let string_value = var_value.to_string();
            let final_string = functions.iter().try_fold(string_value, |acc, f| {
                f.apply(acc)
                    .map_err(|e| AgentTypeError::RenderingTemplate(e.to_string()))
            })?;
            Ok(serde_json::Value::String(final_string))
        }
    }
}
```

Import `SupportedFunction` and `Function` at the top of the file if not already imported (they are already imported for `template_string`).

- [ ] **Step 4: Run the new tests**

Run: `cargo test -p agent-control --lib agent_type::templates::tests::test_toyaml_pipe`
Expected: PASS.

- [ ] **Step 5: Run the full templates test module**

Run: `cargo test -p agent-control --lib agent_type::templates`
Expected: PASS — every existing test still passes because the fallback branches preserve legacy behavior.

- [ ] **Step 6: Run the full workspace tests**

Run: `cargo test -p agent-control --lib`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add agent-control/src/agent_type/templates.rs
git commit -m "$(cat <<'EOF'
feat(agent-type): render toYAML as raw yaml substitution

`template_yaml_value_string` now parses the pipe list of a lone
placeholder. If the pipes contain `toYAML`, the variable's raw yaml
value is substituted in place. All other placeholders continue to use
the legacy type-based branching (unchanged for this commit).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Migrate registry + fixture callsites to add `| toYAML`

**Files:**
- Modify: `agent-control/agent-type-registry/newrelic/*.yaml` (subset that uses tree substitution)
- Modify: `agent-control/tests/**/*.yml`, `test/**/*.yml` (test fixtures)
- Modify: inline agent-type YAML strings in `agent-control/src/agent_type/render.rs`, `definition.rs`, `sub_agent.rs` if they rely on tree substitution.

For every placeholder `${nr-var:X}` (with or without existing pipes, no `toYAML` yet) where variable `X` is declared as `type: yaml` or `type: map[string]yaml`, and the placeholder is either the entire value of a YAML key or appears alone in a `|`-block scalar that expects a mapping/array, add `| toYAML`.

Do **not** remove `type:` fields yet — the legacy branch still needs them.

- [ ] **Step 1: Enumerate the placeholders that need migration**

Run: `grep -rn 'type: yaml\|type: map\[string\]yaml' /Users/pgallina/go/src/github.com/newrelic/agent-control/agent-control/agent-type-registry/newrelic/`
Expected: A list of every variable in the registry declared with a yaml-shaped type. Note the variable name and the file.

Then, for each variable name found, grep the deployment block of the same file for occurrences of `${nr-var:<name>}`:

Run: `grep -n '${nr-var:<name>}' <same-file>`
Expected: Placeholders that need `| toYAML` appended.

Repeat for `agent-control/tests/` and `test/` YAML fixtures:
```
grep -rn 'type: yaml\|type: map\[string\]yaml' /Users/pgallina/go/src/github.com/newrelic/agent-control/agent-control/tests/ /Users/pgallina/go/src/github.com/newrelic/agent-control/test/
```

Track the list. Skip any variable that never appears in a placeholder (e.g. only used in tests as data).

- [ ] **Step 2: Apply the migration to the newrelic K8s agent-types**

For each file listed in Step 1's registry results, rewrite affected placeholders. Example (`agent-control/agent-type-registry/newrelic/kubernetes-com.newrelic.apm_java-0.1.0.yaml`):

Before:
```yaml
resources: ${nr-var:resourceRequirements}
securityContext: ${nr-var:securityContext}
env: ${nr-var:env}
```

After:
```yaml
resources: ${nr-var:resourceRequirements | toYAML}
securityContext: ${nr-var:securityContext | toYAML}
env: ${nr-var:env | toYAML}
```

Apply the same pattern to every `apm_*` file, `kubernetes-com.newrelic.*` file, and any host agent-type that declares a yaml-typed variable and interpolates it inline.

**Do not touch** placeholders inside multi-line block scalars (e.g. `| indent N` cases) — those are string-form; leave them alone.

**Do not touch** placeholders where the variable is `type: string`, `type: bool`, `type: number` — those already stringify correctly.

- [ ] **Step 3: Apply the same migration to test fixtures**

Same rule for every YAML fixture found in Step 1:
- `agent-control/tests/k8s/data/**/*.yml`
- `test/k8s-e2e/dynamic/*.yml`
- `agent-control/tests/on_host/**/*.yml`

For each file, add `| toYAML` to placeholders that reference yaml-typed variables inline.

- [ ] **Step 4: Apply the migration to Rust inline YAML constants**

Search Rust files for inline agent-type YAML strings:

```
grep -rn 'type: yaml\|type: map\[string\]yaml' /Users/pgallina/go/src/github.com/newrelic/agent-control/agent-control/src/ /Users/pgallina/go/src/github.com/newrelic/agent-control/agent-control/tests/
```

Update inline constants in `agent-control/src/agent_type/render.rs` (e.g. `K8S_AGENT_TYPE_YAML_VARIABLES`), `agent-control/src/agent_type/definition.rs`, and `agent-control/src/sub_agent.rs` where the deployment block interpolates a yaml-typed variable. Add `| toYAML` to those placeholders.

Also update the **expected output** strings in those tests only if the current expected output is a yaml tree — no change needed if the current expected output was already a tree (since we're preserving behavior).

- [ ] **Step 5: Run the full workspace tests**

Run: `cargo test -p agent-control`
Expected: PASS — the pipe is honored by the new toYAML branch; behavior is unchanged.

- [ ] **Step 6: Commit**

```bash
git add agent-control/agent-type-registry/ agent-control/tests/ test/ agent-control/src/
git commit -m "$(cat <<'EOF'
refactor(agent-type): opt yaml-typed placeholders into toYAML pipe

Adds `| toYAML` to every placeholder that referenced a yaml-typed
variable inline. Behavior is unchanged for this commit — the toYAML
branch and the legacy type-based branch both produce a raw yaml tree.
Prepares the codebase for the removal of variable types.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Add the `render_as_string` helper

**Files:**
- Modify: `agent-control/src/agent_type/templates.rs`

Add the helper that will replace `TrivialValue::Display` in Task 5.

- [ ] **Step 1: Add failing tests**

Append to `tests` module of `agent-control/src/agent_type/templates.rs`:

```rust
#[rstest]
#[case::string(serde_json::Value::String("foo".into()), "foo")]
#[case::bool_true(serde_json::Value::Bool(true), "true")]
#[case::bool_false(serde_json::Value::Bool(false), "false")]
#[case::number_int(serde_json::json!(42), "42")]
#[case::number_float(serde_json::json!(3.14), "3.14")]
#[case::null(serde_json::Value::Null, "")]
fn test_render_as_string_scalars(#[case] input: serde_json::Value, #[case] expected: &str) {
    assert_eq!(render_as_string(&input), expected);
}

#[test]
fn test_render_as_string_map() {
    let input = serde_json::json!({"key": "value"});
    let s = render_as_string(&input);
    // serde_saphyr renders as "key: value\n" (trailing newline).
    assert!(s.contains("key: value"), "unexpected: {s}");
}

#[test]
fn test_render_as_string_array() {
    let input = serde_json::json!(["a", "b"]);
    let s = render_as_string(&input);
    assert!(s.contains("- a"), "unexpected: {s}");
    assert!(s.contains("- b"), "unexpected: {s}");
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p agent-control --lib agent_type::templates::tests::test_render_as_string`
Expected: FAIL — `render_as_string` is not defined.

- [ ] **Step 3: Add the helper**

Add near the top of `agent-control/src/agent_type/templates.rs`, after the imports:

```rust
/// Renders a YAML value as a string:
/// - Scalars use their bare textual form (`true`, `42`, `foo`).
/// - Null renders as an empty string.
/// - Maps and arrays are serialized to multi-line YAML text via `serde_saphyr`.
pub fn render_as_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Null => String::new(),
        _ => serde_saphyr::to_string(value)
            .expect("serde_json::Value is always YAML-serializable"),
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p agent-control --lib agent_type::templates::tests::test_render_as_string`
Expected: PASS.

- [ ] **Step 5: Run the full workspace tests**

Run: `cargo test -p agent-control --lib`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add agent-control/src/agent_type/templates.rs
git commit -m "$(cat <<'EOF'
feat(agent-type): add render_as_string helper

Introduces a helper that converts any serde_json::Value into its
render-string form (scalars bare, complex values as YAML text). Will
replace TrivialValue::Display in the model rewrite.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Rewrite the variable model (delete typing, single untyped Variable)

**Files:**
- Delete: `agent-control/src/agent_type/variable/variable_type.rs`
- Delete: `agent-control/src/agent_type/variable/fields.rs`
- Delete: `agent-control/src/agent_type/trivial_value.rs`
- Rewrite: `agent-control/src/agent_type/variable.rs`
- Rewrite: `agent-control/src/agent_type/variable/variants.rs`
- Modify: `agent-control/src/agent_type/templates.rs`
- Modify: `agent-control/src/agent_type/definition.rs`
- Modify: `agent-control/src/agent_type/render.rs`
- Modify: `agent-control/src/agent_type.rs` (module declarations)
- Modify: any file that references `TrivialValue`, `VariableTypeDefinition`, `VariableType`, `Fields<T>`, `StringFields`, `StringFieldsDefinition`, `YamlFieldsDefinition`, `FieldsDefinition<T>`

This is one atomic commit — the Rust compiler cannot reach a green state until the whole model is coherent.

- [ ] **Step 1: Audit call-sites of the deleted symbols**

Run:
```
grep -rn 'TrivialValue\|VariableTypeDefinition\|VariableType\b\|StringFieldsDefinition\|YamlFieldsDefinition\|FieldsDefinition\|StringFields\|Fields<' /Users/pgallina/go/src/github.com/newrelic/agent-control/agent-control/src /Users/pgallina/go/src/github.com/newrelic/agent-control/agent-control/tests
```
Expected: A list of every file that mentions these symbols. Every one needs review.

- [ ] **Step 2: Rewrite `variants.rs` to be `serde_json::Value`-based**

Replace `agent-control/src/agent_type/variable/variants.rs` with:

```rust
//! This module defines the type to configure variants which can restrict Agent Type values to a
//! particular collection of supported values.

use serde::{Deserialize, Serialize};

use crate::agent_type::templates::render_as_string;

/// Represents a collection of supported variants for a variable.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct Variants(Vec<serde_json::Value>);

/// Defines the configuration to be set when defining [Variants] from Agent Control configuration.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct VariantsConfig {
    #[serde(default)]
    pub(crate) ac_config_field: Option<String>,
    #[serde(default)]
    pub(crate) values: Variants,
}

impl Variants {
    /// Returns whether `value` is allowed: true if there are no restrictions, or if `value` is one
    /// of the configured variants.
    pub fn is_valid(&self, value: &serde_json::Value) -> bool {
        self.0.is_empty() || self.0.iter().any(|v| v == value)
    }
}

impl From<Vec<serde_json::Value>> for Variants {
    fn from(value: Vec<serde_json::Value>) -> Self {
        Self(value)
    }
}

impl From<Vec<String>> for Variants {
    fn from(value: Vec<String>) -> Self {
        Self(value.into_iter().map(serde_json::Value::String).collect())
    }
}

impl std::fmt::Display for Variants {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let items: Vec<String> = self.0.iter().map(render_as_string).collect();
        write!(f, "[{}]", items.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::default("", Default::default())]
    #[case::values_only(
        r#"{"values": ["v"]}"#,
        VariantsConfig { values: vec!["v".to_string()].into(), ..Default::default()})
    ]
    #[case::ac_config_only(
        r#"{"ac_config_field": "some_variants"}"#,
        VariantsConfig { ac_config_field: Some("some_variants".to_string()), ..Default::default()})
    ]
    #[case::all(
        r#"{"ac_config_field": "some_variants", "values": ["v1", "v2"]}"#,
        VariantsConfig { ac_config_field: Some("some_variants".to_string()), values: vec!["v1".to_string(), "v2".to_string()].into()})
    ]
    fn test_variants_config_deserialization(
        #[case] input: &str,
        #[case] expected: VariantsConfig,
    ) {
        let value: VariantsConfig = serde_saphyr::from_str(input).unwrap();
        assert_eq!(value, expected);
    }

    #[test]
    fn test_is_valid_with_yaml_values() {
        let variants: Variants = vec![
            serde_json::json!(1),
            serde_json::json!("two"),
            serde_json::json!(true),
        ]
        .into();
        assert!(variants.is_valid(&serde_json::json!(1)));
        assert!(variants.is_valid(&serde_json::json!("two")));
        assert!(variants.is_valid(&serde_json::json!(true)));
        assert!(!variants.is_valid(&serde_json::json!("nope")));
    }
}
```

- [ ] **Step 3: Rewrite `variable.rs`**

Replace `agent-control/src/agent_type/variable.rs` with:

```rust
//! Agent-Type variable definition and its runtime counterpart.
//!
//! Variables are untyped: the user may supply any YAML value. The renderer stringifies by default;
//! the `| toYAML` pipe (see `templates.rs`) opts into raw YAML substitution.

pub mod constraints;
pub mod namespace;
pub mod secret_variables;
pub mod tree;
pub mod variants;

use serde::{Deserialize, Serialize};

use crate::agent_type::{
    error::AgentTypeError,
    variable::{
        constraints::{VariableConstraints, VariantsConstraints},
        variants::{Variants, VariantsConfig},
    },
};

/// Static Variable definition — the shape deserialized from Agent Type YAML.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct VariableDefinition {
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) required: bool,
    #[serde(default)]
    pub(crate) default: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) variants: VariantsConfig,
}

/// [VariableDefinition] including information known at runtime.
#[derive(Debug, PartialEq, Clone)]
pub struct Variable {
    pub(crate) description: String,
    pub(crate) required: bool,
    pub(crate) default: Option<serde_json::Value>,
    pub(crate) final_value: Option<serde_json::Value>,
    pub(crate) variants: Variants,
}

impl VariableDefinition {
    /// Returns the corresponding [Variable] according to the provided configuration.
    pub fn with_config(self, constraints: &VariableConstraints) -> Variable {
        let variants = build_variants(self.variants, &constraints.variants);
        Variable {
            description: self.description,
            required: self.required,
            default: self.default,
            final_value: None,
            variants,
        }
    }
}

fn build_variants(config: VariantsConfig, constraints: &VariantsConstraints) -> Variants {
    let Some(ac_config_field) = config.ac_config_field.as_ref() else {
        return config.values;
    };
    let Some(supported_values) = constraints.get(ac_config_field) else {
        tracing::debug!(
            %ac_config_field,
            "The variants pointed in Agent Type are not set in Agent Control configuration, using defaults"
        );
        return config.values;
    };
    supported_values.into()
}

impl Variable {
    /// Builds a string variable already populated with its final value.
    pub fn new_final_string_variable(final_value: impl ToString) -> Self {
        Self {
            description: String::new(),
            required: false,
            default: None,
            final_value: Some(serde_json::Value::String(final_value.to_string())),
            variants: Variants::default(),
        }
    }

    /// Returns whether this variable must be provided with a value.
    pub fn is_required(&self) -> bool {
        self.required
    }

    /// Returns the variable's final value (its set value, or its default), if any.
    pub fn get_final_value(&self) -> Option<serde_json::Value> {
        self.final_value.clone().or_else(|| self.default.clone())
    }

    /// Sets the variable's final value from the given YAML value, checking variants if any.
    pub fn merge_with_yaml_value(
        &mut self,
        yaml: serde_json::Value,
    ) -> Result<(), AgentTypeError> {
        if !self.variants.is_valid(&yaml) {
            return Err(AgentTypeError::InvalidVariant(self.variants.to_string()));
        }
        self.final_value = Some(yaml);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::agent_type::variable::tree::Tree;
    use crate::agent_type::variable::variants::{Variants, VariantsConfig};

    impl Variable {
        pub(crate) fn new(
            description: String,
            required: bool,
            default: Option<serde_json::Value>,
            final_value: Option<serde_json::Value>,
        ) -> Self {
            Self {
                description,
                required,
                default,
                final_value,
                variants: Variants::default(),
            }
        }

        pub(crate) fn new_string(
            description: String,
            required: bool,
            default: Option<String>,
            final_value: Option<String>,
        ) -> Self {
            Self {
                description,
                required,
                default: default.map(serde_json::Value::String),
                final_value: final_value.map(serde_json::Value::String),
                variants: Variants::default(),
            }
        }
    }

    #[test]
    fn variable_definition_tree_deserialize() {
        let value = r#"
foo:
  bar:
    var_name:
      description: "some description"
      required: false
      default: "a"
      variants:
        ac_config_field: "foo.bar.var_name"
        values: ["a", "b"]
"#;
        let tree: Tree<VariableDefinition> = serde_saphyr::from_str(value).unwrap();
        let expected: Tree<VariableDefinition> = Tree::Mapping(HashMap::from([(
            "foo".to_string(),
            Tree::Mapping(HashMap::from([(
                "bar".to_string(),
                Tree::Mapping(HashMap::from([(
                    "var_name".to_string(),
                    Tree::End(VariableDefinition {
                        description: "some description".to_string(),
                        required: false,
                        default: Some(serde_json::Value::String("a".into())),
                        variants: VariantsConfig {
                            ac_config_field: Some("foo.bar.var_name".to_string()),
                            values: vec!["a".to_string(), "b".to_string()].into(),
                        },
                    }),
                )])),
            )])),
        )]));
        assert_eq!(tree, expected);
    }

    #[test]
    fn variable_definition_ignores_legacy_type_field() {
        let yaml = r#"
description: "legacy"
type: yaml
required: false
default: {}
"#;
        let def: VariableDefinition = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(def.description, "legacy");
        assert_eq!(def.default, Some(serde_json::json!({})));
    }
}
```

- [ ] **Step 4: Delete the old files and update the module tree**

Delete:
- `agent-control/src/agent_type/variable/variable_type.rs`
- `agent-control/src/agent_type/variable/fields.rs`
- `agent-control/src/agent_type/trivial_value.rs`

Run:
```bash
rm agent-control/src/agent_type/variable/variable_type.rs
rm agent-control/src/agent_type/variable/fields.rs
rm agent-control/src/agent_type/trivial_value.rs
```

Update `agent-control/src/agent_type.rs` — remove any `pub mod trivial_value;` declaration. The `variable.rs` file already lost its `variable_type` and `fields` pub mods.

Search for stray `pub mod` references:
```
grep -rn 'mod variable_type\|mod fields\|mod trivial_value' /Users/pgallina/go/src/github.com/newrelic/agent-control/agent-control/src/
```
Remove any hits.

- [ ] **Step 5: Simplify `template_yaml_value_string`**

In `agent-control/src/agent_type/templates.rs`, delete the imports for the removed symbols and remove the legacy type-based branch. The final function reads:

```rust
fn template_yaml_value_string(
    s: String,
    variables: &Variables,
) -> Result<serde_json::Value, AgentTypeError> {
    if !only_template_var_re().is_match(s.as_str()) {
        let templated = template_string(s, variables)?;
        return Ok(serde_json::Value::String(templated));
    }
    let captures = template_re()
        .captures(s.as_str())
        .expect("only_template_var_re matched; template_re must too");
    let var_ref = captures.get(1).unwrap().as_str();
    let pipe_str = captures.get(2).map(|m| m.as_str()).unwrap_or("");

    let functions = SupportedFunction::parse_function_list(pipe_str)
        .map_err(|e| AgentTypeError::RenderingTemplate(e.to_string()))?;

    let var_spec = normalized_var(var_ref, variables)?;
    let var_value = var_spec
        .get_final_value()
        .ok_or(AgentTypeError::MissingValue(var_ref.to_string()))?;

    let has_toyaml = functions
        .iter()
        .any(|f| matches!(f, SupportedFunction::ToYaml(_)));
    if has_toyaml {
        if functions.len() > 1 {
            return Err(AgentTypeError::RenderingTemplate(
                "the `toYAML` pipe cannot be combined with other pipes".to_string(),
            ));
        }
        return Ok(var_value);
    }

    let string_value = render_as_string(&var_value);
    let final_string = functions.iter().try_fold(string_value, |acc, f| {
        f.apply(acc)
            .map_err(|e| AgentTypeError::RenderingTemplate(e.to_string()))
    })?;
    Ok(serde_json::Value::String(final_string))
}
```

Also update `template_string` — it uses `normalized_var(...).get_final_value().to_string()` today. Replace with:

```rust
let value = normalized_var
    .get_final_value()
    .ok_or(AgentTypeError::MissingTemplateKey(
        templatable_placeholder.to_string(),
    ))?;
let value = render_as_string(&value);
```

Fix imports at the top: remove `use super::variable::variable_type::VariableType;` and `use super::trivial_value::TrivialValue;`. Keep `SupportedFunction` and `Function`.

- [ ] **Step 6: Rewrite `definition.rs::fill_with_values`**

Locate `fill_with_values` in `agent-control/src/agent_type/definition.rs` (around line 256). Adapt the call to `merge_with_yaml_value` — the signature is unchanged, but the internal validation is now variant-only (no type check). Existing test cases that supplied invalid types for a variable and expected an error must be updated:

- Delete or rewrite tests that asserted "invalid type for a variable" errors — those no longer exist. In particular, the backoff validation tests in `agent-control/src/agent_type/render.rs::test_invalid_values_for_backoff_config` may still succeed because the invalid values fail at variant/downstream deserialization time. Verify each one manually and adjust.
- Update the `test_fill_infra_agent_variables_in` test to assert against `serde_json::Value` instead of `TrivialValue::MapStringYaml`:

```rust
let expected_config_3 = serde_json::json!({
    "log_level": "trace",
    "forward": "true",
});
let expected_status_server = serde_json::json!(8004);
```

- Remove or update the `AGENT_TYPE_WITH_VARIANTS` test that relies on `type: string`; the new variable definition works the same way but the `type:` field is dropped from the constant.

- [ ] **Step 7: Update `render.rs` test constants**

For every inline agent-type YAML constant in `agent-control/src/agent_type/render.rs`:
- Remove the `type: ...` line from each variable definition.
- Ensure every placeholder in the `deployment:` block that was `type: yaml` now has `| toYAML` (this was done in Task 3 but verify — the constants may not have been touched if Task 3's grep missed them).

Verify the `test_render_k8s_config_with_yaml_variables` expected output still parses; the test may need small changes if the pipeline's stringification differs from the pre-rewrite tree substitution.

- [ ] **Step 8: Sweep every remaining reference to deleted symbols**

Run:
```
grep -rn 'TrivialValue\|VariableTypeDefinition\|VariableType\b\|StringFieldsDefinition\|YamlFieldsDefinition\|FieldsDefinition\|StringFields\|Fields<' /Users/pgallina/go/src/github.com/newrelic/agent-control/agent-control/
```

Expected: zero hits in source or tests. Fix every hit by either deleting the reference or replacing with the new types (`serde_json::Value`, `Variable`, `Variants`).

Also verify no dangling imports:
```
cargo check -p agent-control
```

- [ ] **Step 9: Run the full test suite**

Run: `cargo test -p agent-control`
Expected: PASS. If any test fails, the fix is one of:
- Test constant still has `type: X` → remove.
- Test constant expects tree-substitution behavior without `| toYAML` → add the pipe.
- Test asserts `TrivialValue::X` → replace with `serde_json::Value`.

- [ ] **Step 10: Commit**

```bash
git add agent-control/src agent-control/tests
git rm agent-control/src/agent_type/variable/variable_type.rs agent-control/src/agent_type/variable/fields.rs agent-control/src/agent_type/trivial_value.rs 2>/dev/null || true
git commit -m "$(cat <<'EOF'
refactor(agent-type): remove variable typing

Deletes VariableTypeDefinition, VariableType, TrivialValue, and the
FieldsDefinition/Fields family. Agent-type variables no longer declare
a `type:` — any YAML value is accepted at fill time. The renderer
stringifies by default; `| toYAML` opts into raw yaml substitution.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Strip `type:` from registry and fixture YAML files

**Files:**
- Modify: `agent-control/agent-type-registry/newrelic/*.yaml` (21 files, all that contain `type:` under a variable)
- Modify: `agent-control/tests/**/*.yml`, `test/**/*.yml`

Now that the deserializer silently ignores unknown fields, the `type:` lines are dead weight. Remove them for a clean end state.

- [ ] **Step 1: Enumerate every occurrence**

Run:
```
grep -rn '^\s*type: \(string\|bool\|number\|yaml\|map\[string\]yaml\)$' /Users/pgallina/go/src/github.com/newrelic/agent-control/agent-control/agent-type-registry/ /Users/pgallina/go/src/github.com/newrelic/agent-control/agent-control/tests/ /Users/pgallina/go/src/github.com/newrelic/agent-control/test/
```
Expected: A comprehensive list.

- [ ] **Step 2: Delete every matching line**

For each file listed, remove every line matching the pattern above. Preserve indentation for the surrounding block.

Example, `agent-control/agent-type-registry/newrelic/kubernetes-com.newrelic.apm_java-0.1.0.yaml`:

Before:
```yaml
podLabelSelector:
  description: "Pod label selector"
  type: yaml
  default: { }
  required: false
```

After:
```yaml
podLabelSelector:
  description: "Pod label selector"
  default: { }
  required: false
```

- [ ] **Step 3: Verify the deserializer accepts the trimmed files**

Run: `cargo test -p agent-control --lib agent_type::registry`
Expected: PASS — all registry-load tests still succeed.

- [ ] **Step 4: Run the full test suite**

Run: `cargo test -p agent-control`
Expected: PASS.

- [ ] **Step 5: Run the workspace tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add agent-control/agent-type-registry agent-control/tests test
git commit -m "$(cat <<'EOF'
chore(agent-type): strip type: from registry and fixture YAMLs

The variable deserializer no longer inspects `type:` — the field is a
no-op. Remove every occurrence for a clean end state.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: Update user-facing documentation

**Files:**
- Modify: `agent-control/src/agent_type/README.md`
- Modify: `docs/INTEGRATING_AGENTS.md` (if it mentions variable types)
- Modify: any other doc that names the old `type:` field or `yaml`/`map[string]yaml` variants

- [ ] **Step 1: Enumerate the affected docs**

Run:
```
grep -rln 'type: yaml\|type: map\[string\]yaml\|VariableTypeDefinition' /Users/pgallina/go/src/github.com/newrelic/agent-control/docs/ /Users/pgallina/go/src/github.com/newrelic/agent-control/agent-control/src/agent_type/README.md
```
Expected: A list of docs to edit.

- [ ] **Step 2: Rewrite the variable-definition section**

For each doc:
- Remove the "supported types" section (or replace with a note that variables are untyped).
- Update any variable-definition example to drop `type:`.
- Add a short section on the `| toYAML` pipe with one before/after example (copy from the design doc).

- [ ] **Step 3: Verify docs render correctly**

If the repo has any doc-build (`mdbook`, `mkdocs`, etc.), run it locally. Otherwise, eyeball a diff.

- [ ] **Step 4: Commit**

```bash
git add docs/ agent-control/src/agent_type/README.md
git commit -m "$(cat <<'EOF'
docs(agent-type): document untyped variables and toYAML pipe

Updates the agent-type documentation to reflect the removed `type:`
field and introduces the `| toYAML` pipe.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Verification Checklist

After all tasks are done, verify:

- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --workspace -- -D warnings` passes.
- [ ] `grep -rn 'VariableTypeDefinition\|TrivialValue' agent-control/src agent-control/tests` returns nothing.
- [ ] `grep -rn 'type: yaml\|type: map\[string\]yaml\|type: string\|type: bool\|type: number' agent-control/agent-type-registry agent-control/tests test docs` returns nothing (except in doc examples explicitly marked as legacy).
- [ ] A manual smoke test on any K8s agent-type: render it with sample values and confirm the produced manifest is a valid K8s object (mapping values are maps, not strings).

## Rollback Plan

If a K8s manifest turns out broken in a downstream sub-agent after this ships:
- The pattern to look for is `resources: "cpu: 100m\n..."` (a string) where a map was expected.
- Fix by adding `| toYAML` to the missed callsite.
- Revert path: `git revert` Tasks 5 and 6 restores the type-based renderer. Tasks 1-4 are additive and safe to leave.
