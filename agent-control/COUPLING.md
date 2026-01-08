# Coupling Analysis Report

## Executive Summary

**Health Grade**: 🟡 C (Room for improvement)

| Metric | Value |
|--------|-------|
| Files Analyzed | 315 |
| Total Modules | 311 |
| Total Couplings | 6931 |
| Balance Score | 0.48/1.00 |
| Balanced | 302 (4%) |
| Needs Refactoring | 1206 |

**⚠️ Action Required**

- 🟠 **93 High** priority issues should be addressed soon
- 🟡 43 Medium priority issues to review

## 🔧 Refactoring Priorities

### Immediate Actions

**1. 🟠 `newrelic_agent_control::agent_control::resource_cleaner::k8s_garbage_collector` → `newrelic_agent_control::k8s`**

- **Issue**: Cascading Change Risk - Intrusive coupling to frequently-changed component newrelic_agent_control::k8s
- **Why**: Strongly coupling to volatile components means changes will cascade through the system, requiring updates in many places.
- **Action**: Add stable interface `K8sInterface`
- **Balance Score**: 0.00

**2. 🟠 `newrelic_agent_control::agent_control::config` → `newrelic_agent_control::sub_agent`**

- **Issue**: Cascading Change Risk - Intrusive coupling to frequently-changed component newrelic_agent_control::sub_agent
- **Why**: Strongly coupling to volatile components means changes will cascade through the system, requiring updates in many places.
- **Action**: Add stable interface `Sub_agentInterface`
- **Balance Score**: 0.00

**3. 🟠 `newrelic_agent_control::agent_control::http_server::runner` → `newrelic_agent_control::agent_control`**

- **Issue**: Cascading Change Risk - Intrusive coupling to frequently-changed component newrelic_agent_control::agent_control
- **Why**: Strongly coupling to volatile components means changes will cascade through the system, requiring updates in many places.
- **Action**: Add stable interface `Agent_controlInterface`
- **Balance Score**: 0.00

**4. 🟠 `newrelic_agent_control::agent_control::http_server::status` → `newrelic_agent_control::agent_control`**

- **Issue**: Cascading Change Risk - Intrusive coupling to frequently-changed component newrelic_agent_control::agent_control
- **Why**: Strongly coupling to volatile components means changes will cascade through the system, requiring updates in many places.
- **Action**: Add stable interface `Agent_controlInterface`
- **Balance Score**: 0.00

**5. 🟠 `newrelic_agent_control::agent_control::http_server::status` → `newrelic_agent_control::agent_control`**

- **Issue**: Cascading Change Risk - Intrusive coupling to frequently-changed component newrelic_agent_control::agent_control
- **Why**: Strongly coupling to volatile components means changes will cascade through the system, requiring updates in many places.
- **Action**: Add stable interface `Agent_controlInterface`
- **Balance Score**: 0.00

## Issues by Category

### High Efferent Coupling (38 instances)

> A module depending on too many others is fragile and hard to test. Changes anywhere affect this module.

| Severity | Source | Target | Action |
|----------|--------|--------|--------|
| High | `...control::agent_control` | `51 dependencies` | Split into modules: newrelic_agent_contr... |
| High | `...ub_agent::k8s::builder` | `51 dependencies` | Split into modules: newrelic_agent_contr... |
| High | `..._control::run::on_host` | `53 dependencies` | Split into modules: newrelic_agent_contr... |
| High | `...gent::on_host::builder` | `63 dependencies` | Split into modules: newrelic_agent_contr... |
| High | `...ent_control::sub_agent` | `51 dependencies` | Split into modules: newrelic_agent_contr... |

*...and 33 more instances*

### Cascading Change Risk (70 instances)

> Strongly coupling to volatile components means changes will cascade through the system, requiring updates in many places.

| Severity | Source | Target | Action |
|----------|--------|--------|--------|
| High | `...:k8s_garbage_collector` | `...lic_agent_control::k8s` | Add stable interface `K8sInterface` |
| High | `...:agent_control::config` | `...ent_control::sub_agent` | Add stable interface `Sub_agentInterface... |
| High | `...l::http_server::runner` | `...control::agent_control` | Add stable interface `Agent_controlInter... |
| High | `...l::http_server::status` | `...control::agent_control` | Add stable interface `Agent_controlInter... |
| High | `...l::http_server::status` | `...control::agent_control` | Add stable interface `Agent_controlInter... |

*...and 65 more instances*

### High Afferent Coupling (18 instances)

> A module that many others depend on is hard to change. Any modification risks breaking dependents.

| Severity | Source | Target | Action |
|----------|--------|--------|--------|
| High | `102 dependents` | `...lic_agent_control::k8s` | Introduce trait `K8sInterface` with meth... |
| High | `118 dependents` | `...ent_control::sub_agent` | Introduce trait `Sub_agentInterface` wit... |
| High | `181 dependents` | `...gent_control::checkers` | Introduce trait `CheckersInterface` with... |
| High | `109 dependents` | `newrelic_agent_control::*` | Introduce trait `*Interface` with method... |
| High | `372 dependents` | `...control::agent_control` | Introduce trait `Agent_controlInterface`... |

*...and 13 more instances*

### Global Complexity (8 instances)

> Strong coupling to distant components increases cognitive load and makes the system harder to understand and modify.

| Severity | Source | Target | Action |
|----------|--------|--------|--------|
| Medium | `...type::variable::fields` | `..._agent_control::Fields` | Introduce trait `FieldsTrait` with metho... |
| Medium | `...untime_config::on_host` | `...control::health_config` | Introduce trait `Health_configTrait` wit... |
| Medium | `...agent_type::definition` | `...ontrol::runtime_config` | Introduce trait `Runtime_configTrait` wi... |
| Medium | `...:health::on_host::file` | `...rol::FileHealthChecker` | Introduce trait `FileHealthCheckerTrait`... |
| Medium | `...::config_gen::identity` | `...ic_agent_control::Args` | Introduce trait `ArgsTrait` with methods... |

*...and 3 more instances*

### God Module (2 instances)

> Module has too many responsibilities - too many functions, types, or implementations. Consider splitting into focused, cohesive modules. (SRP violation)

| Severity | Source | Target | Action |
|----------|--------|--------|--------|
| Medium | `agent_control::config` | `...ions, 8 types, 7 impls` | Split into modules: agent_control::confi... |
| Medium | `sub_agent` | `...ons, 6 types, 10 impls` | Split into modules: sub_agent_core, sub_... |

## Coupling Distribution

### By Integration Strength

| Strength | Count | % | Description |
|----------|-------|---|-------------|
| Contract | 258 | 4% | Depends on traits/interfaces only |
| Model | 3024 | 44% | Uses data types/structs |
| Functional | 3309 | 48% | Calls specific functions |
| Intrusive | 340 | 5% | Accesses internal details |

### By Distance

| Distance | Count | % |
|----------|-------|---|
| Same Module (close) | 1859 | 27% |
| Different Module | 314 | 5% |
| External Crate (far) | 4758 | 69% |

### By Volatility (Internal Couplings)

| Volatility | Count | % | Impact on Balance |
|------------|-------|---|-------------------|
| Low (rarely changes) | 122 | 6% | No penalty |
| Medium (sometimes changes) | 866 | 40% | Moderate penalty |
| High (frequently changes) | 1185 | 55% | Significant penalty |

### Worst Balanced Couplings

| Source | Target | Strength | Distance | Volatility | Score | Status |
|--------|--------|----------|----------|------------|-------|--------|
| `...rios::infra_agent` | `...data::recipe_data` | Intrusive | External | Med | 0.00 | 🔴 Critical |
| `fs::file_reader` | `...le::MockLocalFile` | Intrusive | External | Med | 0.00 | 🔴 Critical |
| `fs::file_renamer` | `...le::MockLocalFile` | Intrusive | External | Med | 0.00 | 🔴 Critical |
| `fs::writer_file` | `LocalFile::LocalFile` | Intrusive | External | Med | 0.00 | 🔴 Critical |
| `fs::writer_file` | `...le::MockLocalFile` | Intrusive | External | Med | 0.00 | 🔴 Critical |
| `...directory_manager` | `...kDirectoryManager` | Intrusive | External | Med | 0.00 | 🔴 Critical |
| `fs::win_permissions` | `...-sys::windows_sys` | Intrusive | External | Med | 0.00 | 🔴 Critical |
| `fs::win_permissions` | `...-sys::windows_sys` | Intrusive | External | Med | 0.00 | 🔴 Critical |
| `...ration::converter` | `...tion_agent_config` | Intrusive | External | Med | 0.00 | 🔴 Critical |
| `...ration::converter` | `file_info::file_info` | Intrusive | External | Med | 0.00 | 🔴 Critical |
| `...migration::config` | `...:migration_config` | Intrusive | External | Med | 0.00 | 🔴 Critical |
| `...ent_config_getter` | `...ol_dynamic_config` | Intrusive | External | Med | 0.00 | 🔴 Critical |
| `...gration::migrator` | `...g::sub_agents_cfg` | Intrusive | External | Med | 0.00 | 🔴 Critical |
| `...garbage_collector` | `...gent_control::k8s` | Intrusive | Same Mod | High | 0.00 | 🔴 Critical |
| `...:config_validator` | `...g::dynamic_config` | Intrusive | External | Med | 0.00 | 🔴 Critical |

*Showing 15 of 6931 couplings*

## Module Statistics

| Module | Trait Impl | Inherent Impl | Internal Deps | External Deps |
|--------|------------|---------------|---------------|---------------|
| `sub_agent::k8s::supervisor` | 2 | 1 | 57 | 8 |
| `agent_control` | 0 | 3 | 50 | 15 |
| `sub_agent` | 5 | 5 | 52 | 12 |
| `agent_control::run::k8s` | 0 | 1 | 55 | 4 |
| `sub_agent::on_host::builder` | 2 | 1 | 53 | 6 |
| `sub_agent::k8s::builder` | 2 | 1 | 44 | 5 |
| `agent_control::run::on_host` | 0 | 1 | 43 | 4 |
| `sub_agent::on_host::supervisor` | 3 | 2 | 41 | 5 |
| `...ers::version::k8s::checkers` | 2 | 2 | 30 | 6 |
| `agent_control::config` | 7 | 0 | 25 | 8 |
| `on_host::file_store` | 5 | 5 | 24 | 8 |
| `...:effective_agents_assembler` | 3 | 4 | 27 | 4 |
| `...pe::runtime_config::on_host` | 3 | 0 | 28 | 3 |
| `checkers::guid::k8s::checker` | 1 | 1 | 24 | 6 |
| `cloud::cloud_id::detector` | 1 | 1 | 26 | 3 |
| `...oring_gen::infra_config_gen` | 1 | 1 | 26 | 3 |
| `...http_server::status_updater` | 0 | 1 | 22 | 5 |
| `opamp::callbacks` | 1 | 2 | 21 | 5 |
| `...ators::signature::validator` | 2 | 1 | 20 | 6 |
| `agent_control::run` | 2 | 1 | 22 | 3 |

*Showing top 20 of 311 modules*

## Volatility Analysis

### High Volatility Files

⚠️ Strong coupling to these files increases cascading change risk.

| File | Changes |
|------|---------|
| `agent-control/src/agent_control/run/k8s.rs` | 30 |
| `agent-control/src/sub_agent/on_host/supervisor.rs` | 21 |
| `agent-control/src/sub_agent.rs` | 20 |
| `agent-control/src/opamp/remote_config/validators/signature/validator.rs` | 17 |
| `agent-control/src/agent_control/config.rs` | 17 |
| `agent-control/src/sub_agent/effective_agents_assembler.rs` | 17 |
| `agent-control/src/agent_control/run/on_host.rs` | 16 |
| `agent-control/src/agent_control/defaults.rs` | 15 |
| `agent-control/src/sub_agent/on_host/builder.rs` | 14 |
| `agent-control/src/agent_control.rs` | 12 |

## ⚠️ Circular Dependencies

Found **5 circular dependency cycle(s)** involving **5 modules**.

Circular dependencies make code harder to understand, test, and maintain.
Consider breaking cycles by:

1. Extracting shared types into a separate module
2. Inverting dependencies using traits/interfaces
3. Moving functionality to reduce coupling

### Detected Cycles

1. `newrelic_agent_control::event → newrelic_agent_control::sub_agent` → `newrelic_agent_control::event`
2. `newrelic_agent_control::agent_control → newrelic_agent_control::event → newrelic_agent_control::sub_agent → newrelic_agent_control::values → newrelic_agent_control::data_store` → `newrelic_agent_control::agent_control`
3. `newrelic_agent_control::agent_control → newrelic_agent_control::event → newrelic_agent_control::sub_agent → newrelic_agent_control::values` → `newrelic_agent_control::agent_control`
4. `newrelic_agent_control::agent_control → newrelic_agent_control::event → newrelic_agent_control::sub_agent` → `newrelic_agent_control::agent_control`
5. `newrelic_agent_control::agent_control → newrelic_agent_control::event` → `newrelic_agent_control::agent_control`

## Balance Guidelines

The goal is **balanced coupling**, not zero coupling.

### Ideal Patterns ✅

| Pattern | Example | Why It Works |
|---------|---------|--------------|
| Strong + Close | `impl` blocks in same module | Cohesion within boundaries |
| Weak + Far | Trait impl for external crate | Loose coupling across boundaries |

### Problematic Patterns ❌

| Pattern | Problem | Solution |
|---------|---------|----------|
| Strong + Far | Global complexity | Introduce adapter or move closer |
| Strong + Volatile | Cascading changes | Add stable interface |
| Intrusive + Cross-boundary | Encapsulation violation | Extract trait API |

