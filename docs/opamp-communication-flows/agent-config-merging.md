# Merging remote configuration entries

Agent Control receives remote configuration for Agents as [`OpAMP's AgentConfigMap`](https://github.com/open-telemetry/opamp-spec/blob/db1e1fcf14e834469f822496f2fa1ed0512141be/specification.md#configuration-files), a flat map of string keys to string bodies. The role of each entry is determined entirely by its key: Agent Control matches each key against  a set of known prefixes, and those prefixes allow a single payload to carry a base configuration, a full blob-level override, and per-variable overrides all at once.

Agent Control's own configuration follows a different, much simpler set of rules that is described at [Agent Control's own remote configuration](#agent-controls-own-remote-configuration).

For the meaning of the resulting values (agent type variables, their declared `type`, `string_map`, etc.) see [Agent Type Variables](../INTEGRATING_AGENTS.md#agent-type-variables) in [`docs/INTEGRATING_AGENTS.md`](../INTEGRATING_AGENTS.md). For how the merged result is turned into a running agent, see [Applying configurations](../INTEGRATING_AGENTS.md#applying-configurations) in the same document.

## Agent remote configuration

### Config map keys

| Key pattern | Targets |
|---|---|
| `agentConfig[<suffix>]` | A full configuration blob, merged with any other `agentConfig*` entries. |
| `override.agentConfig[<suffix>]` | A full configuration blob that overrides the merged `agentConfig*` result. |
| `variable.agentConfig.<variable-path>` | A single declared variable, addressed by its dot-separated path. |
| `variable.agentConfig.<variable-path>:<key-name>` | A single entry inside a `string_map`-typed variable. |

Any key that doesn't match any of the defined targets is ignored.

### Precedence, from lowest to highest

The four layers are applied in order, each taking precedence over the previous one:

1. **`agentConfig*` entries** are decoded as YAML and merged together. Every entry contributes top-level keys to a single configuration; a key present in more than one entry is an **error**.
2. **`override.agentConfig*` entry** (at most one key is allowed, a second one is an error) is decoded and merged on top of step 1. Unlike step 1, key collisions here are **not** errors: the override always wins.
3. **`variable.agentConfig.<variable-path>` entries** each replace the whole value at their dot-separated path, creating intermediate mappings as needed. Applied after steps 1 and 2.
4. **`variable.agentConfig.<variable-path>:<map-key>` entries** each set a single entry inside the map at `<path>`. Applied last, so a whole-variable override (step 3) and one or more map-entry overrides (step 4) for the **same** path compose: the whole-variable override sets the map's baseline, and the map-entry overrides then add or replace individual entries in it.

### Example

Given an Agent Type defining variables as follows:

```yaml
variables:
  version:
    description: "Agent version"
    type: string
    required: true
  config_agent:
    description: "New Relic infra configuration"
    type: yaml
    required: false
    default: ""
  config_logging:
    description: "map of logging config file names to their contents"
    type: string_map
    required: false
    default: { }
# ...
```

And the following AgentConfigMap entries:

```yaml
# AgentConfigMap entries received via OpAMP
agentConfig-base: |
  config_agent:
    license_key: "abc123"

agentConfig-version: |
  version: "1.48.0"

override.agentConfig: |
  version: "1.80.0"

variable.agentConfig.config_logging: |
  fb-nri.conf: |
    [OUTPUT]
        Name  file
        Match *
        File  /tmp/fb-nri.log

variable.agentConfig.config_logging:fb-extra.conf: |
  [OUTPUT]
      Name  stdout
      Match *

variable.agentConfig.not_defined: | # Ignored as not defined in the Agent Type
  Some content
```

This will be the resulting configuration for the agent:

```yaml
version: "1.80.0"                # step 2 replaced the base value
config_agent:
  license_key: "abc123"
config_logging:
  fb-nri.conf: |                 # step 3 set the map's baseline
    [OUTPUT]
        Name  file
        Match *
        File  /tmp/fb-nri.log
  fb-extra.conf: |               # step 4 added an entry to that same map
    [OUTPUT]
        Name  stdout
        Match *
```

These resulting variable values will be used to deploy the agent as defined in the `deployment` section of the Agent Type.

### How a per-variable override's raw text is interpreted

Steps 3 and 4 both receive a *raw string* from OpAMP and need to turn it into a typed value before it can be merged into the configuration. Agent Control resolves the variable's declared type from the agent type registry and interprets the raw text accordingly:

- **Whole-variable overrides (step 3)**:
  - If the variable is declared `string` (`version` in the example above), the raw text is stored **as-is**, with no YAML parsing. This lets you override a `string` variable with a value that would not otherwise be valid YAML.
  - For every other declared type (`bool`, `number`, `string_map`, `yaml`), the raw text **must be valid YAML**; an override for `config_agent` (a `yaml` variable) that isn't parseable YAML is rejected with an error.
  - A path that doesn't match any declared variable of the resolved agent type is **not an error**: it's ignored. Agent Control logs a `warn!` message to flag the unexpected key.
- **Map-entry overrides (step 4)** additionally require the target variable to be declared `string_map` (an error otherwise, e.g. attempting `variable.agentConfig.version:foo` against the `string` variable `version`). The raw text is parsed as YAML on a best-effort basis: if it parses, the parsed value is stored (so a value like `content: whatever` becomes a nested mapping); if it doesn't parse, the raw text is kept as a plain string, which is what makes this syntax usable to inject a literal file's content (e.g. a fluent bit config, a shell script) into a `string_map` such as `config_logging` or `files` without needing it to double as valid YAML. Like step 3, an unknown path is ignored with a `warn!` log rather than failing.
- An empty map-entry key (`variable.agentConfig.config_logging:`) is rejected before the registry is even consulted.

## Agent Control's own remote configuration

Agent Control's own dynamic configuration is also delivered as an `AgentConfigMap`, and reuses the *same* `agentConfig*` merge as step 1 above: every `agentConfig*` entry is decoded as YAML and merged into a single configuration, with duplicate top-level keys treated as an error.

That is where the similarity ends:
- **`override.agentConfig`, `variable.agentConfig.<path>`, and `variable.agentConfig.<path>:<map-key>` are not recognized at all for AC's own configuration.** Any such key sent alongside AC's `agentConfig*` entries is ignored. There is no blob-level override and no per-variable override mechanism for AC itself, only the base merge.
- The `agents` key gets special handling: instead of being merged as an ordinary YAML value (which would make a duplicate `agents` key across two `agentConfig*` entries an unconditional error), the maps under `agents` from every entry are combined agent-by-agent, and only a duplicate *agent ID* across entries is an error. This is what lets Fleet Control split the desired agent list across multiple `agentConfig*` entries, e.g. one per team or one per templated source.
- Every other top-level key (`chart_version`, `cd_chart_version`, etc.) is merged like step 1 for sub-agents: a plain top-level YAML merge where a duplicate key across entries is an error.

Example:

```yaml
agentConfig-infra: |
  agents:
    agentInfra: newrelic/com.newrelic.infrastructure:0.1.0
agentConfig-nri-redis: |
  agents:
    agentNriRedis: newrelic/com.newrelic.infrastructure.nri_redis:0.1.0
# agentConfig-infra-2: | # This would fail because `agentInfra` would be repeated
#   agents:
#     agentInfra: newrelic/com.newrelic.infrastructure:0.1.0
override.agentConfig: | # This is ignored as any key not prefixed by `agentConfig`
  agents:
    testingAgent: namespace/name:0.0.100
```

Will become a remote configuration for Agent Control:

```yaml
agents:
  agentInfra: newrelic/com.newrelic.infrastructure:0.1.0
  agentNriRedis: newrelic/com.newrelic.infrastructure.nri_redis:0.1.0
```

## See also

- [`docs/opamp-communication-flows/subagent-remote-deployment.md`](subagent-remote-deployment.md): how the merged config affects whether a sub-agent starts, stops, or waits.
- [`docs/opamp-communication-flows/delete-configuration.md`](delete-configuration.md): what an empty `AgentConfigMap` entry means, and how that differs from simply omitting a key.
- [`docs/INTEGRATING_AGENTS.md`](../INTEGRATING_AGENTS.md): agent type variables, their declared types, and how they're consumed by an agent type's deployment definition.
- [`docs/CONFIG.md`](../CONFIG.md): Agent Control's own (non-remote) configuration fields.
