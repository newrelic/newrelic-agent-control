# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Unreleased section should follow [Release Toolkit](https://github.com/newrelic/release-toolkit/blob/main/README.md).

Remember that the keywords that you can use are the following:
 - breaking    => Major
 - security    => Minor
 - enhancement => Minor
 - bugfix      => Patch

## Unreleased

### enhancement
- Added support for JP endpoints.
- Agent type definitions now use a flat per-platform schema (one YAML file per `(platform, operating_system)` pair). 
  Agent type FQNs and configuration values are unchanged, so this is **not a breaking change** for end users.
  Internal authors of custom agent type definitions need to migrate their YAMLs — see [docs/INTEGRATING_AGENTS.md](docs/INTEGRATING_AGENTS.md).
- Introduced ephemeral/persistent lifecycle semantics for sub-agent filesystem directories.
  Ephemeral directories are cleared on agent stop/restart/config updates.
  extra tracking. Persistent directories survive config changes and are only deleted on agent removal — see [docs/INTEGRATING_AGENTS.md](docs/INTEGRATING_AGENTS.md).
- Agent type definitions now declare a top-level `protocol_version` (a quoted `MAJOR.MINOR` string) that versions the
  agent-type schema language itself. It is validated against the version Agent Control supports at registry ingestion,
  so definitions targeting an incompatible schema are rejected early. Internal authors of custom agent type definitions
  must add this field — see [docs/INTEGRATING_AGENTS.md](docs/INTEGRATING_AGENTS.md).

## v1.16.1 - 2026-06-04

### 🐞 Bug fixes
- the agentType for the PCG config should be called pipeline_control_gateway_config_mode

## v1.16.0 - 2026-06-02

### 🚀 Enhancements
- Add support for Ubuntu 26.

### ⛓️ Dependencies
- Updated rust crate serial_test to 3.5.0

## v1.15.2 - 2026-05-29

### 🐞 Bug fixes
- Add DaemonSet health-check in pipeline-control-gateway-config Agent Type
- pcg_config: revert agentType config

### ⛓️ Dependencies
- Updated rust crate opentelemetry_sdk to 0.32.1
- Updated rust crate serde-saphyr to 0.0.27
- Updated rust to v1.96.0

## v1.15.1 - 2026-05-26

### 🐞 Bug fixes
- On-host: clean the OpAMP instance id and remote config of an agent when it is removed, matching the k8s behavior (previously these files were left orphaned under `fleet-data/`).
- self-instrumentation: dropped the internal traces exporter support for self-instrumentation.
- pcg_config: the agentType variable structure should assume the same file structure
- cd_enabled: when cd_enabled is false, we should set ac_remote_update to false as well

### ⛓️ Dependencies
- Updated rust crate oci-client to 0.17.0
- Updated rust crate either to 1.16.0
- Updated rust crate serde_json to 1.0.150
- Updated rust crate http to 1.4.1
- Updated rust crate reqwest to 0.13.4

## v1.15.0 - 2026-05-19

### 🚀 Enhancements
- bumping opamp-rs to fix custom_capabilities

### ⛓️ Dependencies
- Updated rust crate aws-lc-rs to 1.17.0
- Updated rust crate config to 0.15.23
- Updated alpine/helm to v4.2.0
- Updated rust crate opamp-client to v0.0.39
- Updated alpine/kubectl to v1.36.1
- Updated rust crate tar to 0.4.46

## 1.14.1 - 2026-05-13

### 🚀 Enhancements
- We should check cd_enabled not remote update

## 1.14.0 - 2026-05-08

### 🚀 Enhancements
- OCI registry authentication configurable via config
- Adds support for Fluxless mode
