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

### bugfix
- On-host self-update: an empty `version: ""` pushed from Fleet Control now behaves the same as an absent `version` field (no-op, no update attempted). Previously it silently triggered a pull of the `:latest` OCI tag.

### enhancement
- Adds exponential backoff + jitter retries to OCI artifact fetches via a new `BackoffPolicy`
  (configurable under `self_update.download_retry`), replacing the old `with_retries(usize, Duration)` API.
- add post-download script support for OCI packages.
- Hardens service restart policies on Linux and Windows (systemd rate limiting: 5 restarts max in 60s) to prevent crash-looping from saturating CPU.
- Added support for remote agent type definition retrieval from OCI registries.
- Agent type definitions now tolerate unknown fields for forward compatibility.
- On-host agents: removed command-based version checking. Agent version is now determined from OCI package metadata, eliminating the need for `deployment.version` configuration in agent type definitions.
- Replace filesystem in on-host agent-type definitions with an explicit, recursive, tagged-kind tree: every entry declares `kind: file | dir | dir_content_from_map`, and `dir` entries nest via `entries:`.
- On-host filesystem entries now accept a `persistent` flag (default `false`): ephemeral entries are deleted on sub-agent stop, persistent entries survive until the agent is removed from the fleet. Reconciliation across writes is driven by a sidecar `.ac-managed-paths.json` manifest (reserved filename — agent types must not declare it) so paths Agent Control no longer owns are deleted while sub-agent-created files are preserved.

## v1.17.0 - 2026-06-16

### 🚀 Enhancements
- Added support for JP endpoints.
- Agent type definitions changed: flat per-platform schema (one YAML per `(platform, operating_system)` pair), a new top-level `protocol_version` field (quoted `MAJOR.MINOR`, validated at registry ingestion), and stricter `name`/`namespace` validation (no `-`) and `version` (plain `Major.Minor.Patch` semver). FQNs and configuration values are unchanged, so this is **not a breaking change** for end users; internal authors of custom definitions must migrate — see [docs/INTEGRATING_AGENTS.md](docs/INTEGRATING_AGENTS.md).

### 🐞 Bug fixes
- Validate version coming from remote.

### ⛓️ Dependencies
- Updated rust crate chrono to 0.4.45
- Updated amazon-eks to v1.36
- Updated rust crate http to 1.4.2
- Updated rust crate regex to 1.12.4
- Updated alpine/kubectl to v1.36.2
- Updated alpine/helm to v4.2.1
- Updated rust crate opamp-client to v0.0.40
- Updated rust crate nr-auth to v0.5.1

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
