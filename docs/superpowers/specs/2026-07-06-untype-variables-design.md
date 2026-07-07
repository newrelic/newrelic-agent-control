# Untype Agent-Type Variables — remove `VariableTypeDefinition`, add `| toYAML` pipe

**Status:** Draft
**Date:** 2026-07-06

## Summary

Remove the type discrimination on Agent-Type variables. Today, every variable
declares a `type:` field (`string | bool | number | yaml | map[string]yaml`),
and that field drives (a) which YAML shape the user is allowed to supply and
(b) how the rendered value is inserted into the deployment template.

We replace this with:

1. A single untyped variable definition — the user may supply any YAML value.
2. A default rendering strategy that always stringifies the value.
3. A new `| toYAML` pipe that, when present alone on a placeholder that is
   itself alone in a YAML value, substitutes the raw YAML tree in place.

The pipe mirrors Helm's mental model of "opt in to YAML expansion" but with
simpler mechanics (in-place tree substitution instead of Helm's
string-plus-indent recipe).

## Motivation

`VariableTypeDefinition` produces a large surface: each variant carries its
own `Fields<T>` implementation, its own deserializer, its own default rules,
its own path through the renderer. That complexity buys very little — the
main behavioral fork the type controls is "expand this placeholder as a YAML
tree instead of a string." Everything else is validation that the
downstream deserializer already re-does.

Reducing to a single value shape (`serde_json::Value`) plus one explicit
opt-in pipe (`| toYAML`) removes ~200 lines of enum boilerplate, unifies the
value model with `TrivialValue`, and makes rendering rules easier to reason
about.

## Design

### Data model

Delete: `VariableTypeDefinition`, `VariableType`, `TrivialValue`,
`StringFieldsDefinition`, `YamlFieldsDefinition`, `FieldsDefinition<T>`,
`Fields<T>`, `StringFields`.

Collapse to:

```rust
pub struct VariableDefinition {
    pub description: String,
    pub required: bool,
    pub default: Option<serde_json::Value>,
    pub variants: VariantsConfig<serde_json::Value>,
}

pub struct Variable {
    pub description: String,
    pub required: bool,
    pub default: Option<serde_json::Value>,
    pub final_value: Option<serde_json::Value>,
    pub variants: Variants<serde_json::Value>,
}
```

Every runtime value is `serde_json::Value`. A small helper replaces
`TrivialValue::Display`:

```rust
fn render_as_string(v: &serde_json::Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
        // Maps/arrays -> multi-line YAML text
        _ => serde_saphyr::to_string(v)
            .expect("serde_json::Value is always YAML-serializable"),
    }
}
```

### Deserialization behavior

- `#[serde(default)]` on all fields with sensible defaults
  (`required = false`, `default = None`, `variants = empty`).
- The old `type:` field is silently ignored (unknown fields are dropped by
  serde's default behavior — no explicit handling needed).
- The old YAML null-default trick (a `type: yaml` variable with no `default:`
  meant "default null") is dropped. If a variable is not required, its
  `default` must be explicit (including `default: null` or `default: ~`).

### Rendering rules

`template_yaml_value_string(s, vars) -> Result<Value>` branches as follows:

1. **Placeholder not alone in the string** (e.g. `"prefix ${nr-var:foo}"`,
   or a multi-line block scalar with a placeholder inside) → run
   `template_string` and return `Value::String`. Any `| toYAML` in the pipe
   list is a no-op in this branch.

2. **Placeholder alone, pipe list contains `toYAML`** → look up the
   variable, return its `final_value` as `serde_json::Value` directly
   (tree substitution). Other pipes in the list are rejected as
   incompatible with `toYAML`.

3. **Placeholder alone, no `toYAML`** → stringify via `render_as_string`,
   apply remaining pipes (currently `indent`), return `Value::String`.

Concrete effects:

- `resources: ${nr-var:resourceRequirements | toYAML}` with user value
  `{cpu: 100m}` → real map under `resources`.
- `resources: ${nr-var:resourceRequirements}` with the same value →
  `resources: "cpu: 100m\n"` (a string; downstream K8s would reject).
  **This is a breaking change** that requires migration of every K8s
  agent-type that relies on tree substitution.
- `port: ${nr-var:status_server_port}` with numeric `8004` → `port: "8004"`.
  Downstream `TemplateableValue<Port>` already parses from strings, so
  numeric fields keep working.

### `| toYAML` pipe

- New `SupportedFunction::ToYaml` variant, parsed from the token `toYAML`
  (case-insensitive to match the existing `indent` parser).
- `apply(String) -> String` is identity — needed so that string-only
  paths (branch 1 above) don't error when they see `toYAML`.
- The tree-swap behavior lives in `template_yaml_value_string` (branch 2).
- `toYAML` combined with any other pipe (e.g. `| toYAML | indent 2`) is
  rejected with a clear error — tree substitution and string transforms
  don't mix.

### Fill behavior

`fill_with_values` becomes trivial:

- For each variable name found in the user's values tree, take the raw
  `serde_json::Value` and store it as `final_value`.
- If `variants` is non-empty, check that the value appears in the list
  (deep equality on `Value`); otherwise error.
- If the variable is `required` and has no `final_value` after fill,
  error at the existing "not populated" check.

No type validation. Both `foo: "some string"` and `foo: {nested: value}`
are valid for the same variable.

### `variants`

Kept as a feature but generalized: values in the allow-list are
`serde_json::Value`, matched with deep equality. The existing
`ac_config_field` mechanism (allow-list sourced from Agent Control config)
continues to work. `Variants::Display` (used for error messages) formats
each variant with the same `render_as_string` helper.

## Migration

### Registry files

- `agent-control/agent-type-registry/newrelic/*.yaml` (~10 files) — strip
  the `type:` field from every variable, and add `| toYAML` to every
  placeholder in `deployment:` that relies on tree substitution.
- Same treatment for `agent-control/tests/**/*.yml` and `test/**/*.yml`
  test fixtures.

### Rust test constants

`render.rs`, `definition.rs`, `templates.rs`, `variable.rs` all embed
agent-type YAML strings in test constants. Every one needs the same
migration.

### API/CLI consumers

None internal — the Agent-Type YAML schema is the boundary. External
authors of custom agent types must migrate their files (grace period
provided by the "silently ignore `type:`" behavior).

### Documentation

- Update any user-facing docs that describe the variable schema
  (grep `type: yaml` in `docs/`).

## Files affected

**Delete:**
- `agent-control/src/agent_type/variable/variable_type.rs`
- `agent-control/src/agent_type/trivial_value.rs`

**Rewrite:**
- `agent-control/src/agent_type/variable.rs`
- `agent-control/src/agent_type/variable/fields.rs` (or merge into
  `variable.rs` and delete)
- `agent-control/src/agent_type/variable/variants.rs` (specialize to
  `serde_json::Value`)
- `agent-control/src/agent_type/templates.rs`
- `agent-control/src/agent_type/templates_function.rs` (add `ToYaml`)

**Touch:**
- `agent-control/src/agent_type/definition.rs` (fill logic + tests)
- `agent-control/src/agent_type/render.rs` (test constants)
- `agent-control/src/agent_type/variable/tree.rs`,
  `namespace.rs`, `constraints.rs`, `secret_variables.rs` (call-site
  updates as needed)
- `agent-control/agent-type-registry/newrelic/*.yaml`
- `agent-control/tests/**/*.yml`, `test/**/*.yml`

## Testing

- Rendering table tests for the three branches:
  - Not-alone placeholder — always string.
  - Alone + `toYAML` — tree substitution.
  - Alone + no pipe — stringified value.
- Fill tests: same variable filled with both string and mapping succeeds;
  `variants` check works against any YAML value.
- Regression: every existing test in `variable.rs`, `templates.rs`,
  `render.rs`, `definition.rs`.
- Error paths: `toYAML` combined with `indent` errors; missing required
  variable errors as before.

## Risks

- **Silent breakage of K8s manifests** if a registry file is not migrated:
  the AC renderer succeeds (produces a manifest with a string where a
  map/array was expected), but the sub-agent's K8s apply fails. Mitigation:
  the migration PR must touch every registry file at once; CI e2e tests
  should catch anything missed.
- **Inline numeric/bool placeholders** in raw K8s specs (e.g.
  `replicas: ${nr-var:count}`) now emit strings. Mitigation: audit with
  `grep -rn '\${nr-var:[^}]*}'` across the registry; require quoting or
  `| toYAML` where the K8s schema demands a scalar.
- **External custom agent types** authored by end users will start rendering
  their YAML variables as strings by default. The "silently ignore `type:`"
  behavior means the parser doesn't reject them, but their manifests break
  the same way. Release notes must call this out prominently.

## Non-goals

- No changes to secret variables, environment variables, or the namespace
  system.
- No changes to the pipe function grammar beyond adding `toYAML`.
- No new features on `variants` (e.g. no regex, no ranges).
