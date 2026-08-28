# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is this project?

**Agent Control (AC)** is a Rust binary that acts as a lightweight supervisor/control plane for New Relic observability agents. It manages the lifecycle of sub-agents (processes, Kubernetes Helm releases, APM auto-instrumentation) using static local config or remote config pushed from **Fleet Control (FC)** over the [OpAMP protocol](https://github.com/open-telemetry/opamp-spec).

There are two deployment targets, each with its own binary:
- **On-host** (`newrelic-agent-control`): manages sub-agents as OS processes; targets Linux (x86_64/aarch64) and Windows.
- **Kubernetes** (`newrelic-agent-control-k8s`): manages sub-agents as Helm releases (Flux CRs) and Instrumentation CRs.

For the full architecture overview and network requirements, see [`docs/README.md`](docs/README.md). For the on-disk/runtime file layout (config paths, `local-data`, `fleet-data`, etc.), see [`docs/AC_repositories_and_files.md`](docs/AC_repositories_and_files.md).

## Workspace structure

Member crates match `[workspace] members` in the root [`Cargo.toml`](Cargo.toml). `agent-control` is the main crate (both binaries, plus the embedded `agent-type-registry/`); the rest are small support crates and test infra.

For the current module breakdown inside `agent-control/src/`, run `find agent-control/src -maxdepth 1 -type d`, that list changes more often than this file gets updated.

## Development

Building, cross-compilation, running locally, testing (unit, integration, root-required, k8s cluster-required), coverage, and profiling are all documented in [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md). `agent-control` (the `newrelic_agent_control` package) holds most of the test suite; its onhost/k8s/root-required tests run through the dedicated `make -C agent-control test/...` targets described there, not a single blanket `cargo test`.

Before considering work done:
- Always run `cargo clippy --workspace --all-targets` and `cargo build`, plus tests for the modules you touched. This mirrors what CI enforces in [`push_pr_checks_tests.yml`](.github/workflows/push_pr_checks_tests.yml).
- Always check that the code you write abides the code conventions.

## Agent type definitions

`*.yaml` files under `agent-control/agent-type-registry/` are **embedded into the binary at compile time** (via `build.rs`). Any new YAML added there is automatically available at runtime. The registry defines how each sub-agent is deployed, configured, and health-checked.

- Full schema: [`docs/INTEGRATING_AGENTS.md`](docs/INTEGRATING_AGENTS.md)
- Template variable syntax (`${<namespace>:<ref>}`) and the full namespace reference: [`docs/VARIABLE_INTERPOLATION.md`](docs/VARIABLE_INTERPOLATION.md)

## Code conventions

Full style guides live under [`docs/style/`](docs/style/), check that folder for the complete, current set.

Rules worth keeping top of mind while writing code:
- **Errors (`thiserror`)**: messages start lowercase, no trailing period. Start with a struct error; promote to an enum only when callers need to match specific variants. Avoid `#[from]` unless the convenience is worth losing an explicit call site and clippy's unused-variant detection.
- **Logs (`tracing`)**: messages start with a capital letter, no trailing period, with the error included in the message text even though it's otherwise static. Use structured, `snake_case` fields for dynamic content, not format strings. Spans are short-lived and always level `INFO`; a `DEBUG` span silently drops context from its child `INFO` events.
- Comments should be concise and never compare the current approach to a previous one.

## Key external dependencies

See [`Cargo.toml`](Cargo.toml) for the authoritative, current list. Two things aren't obvious from there:
- [`newrelic-opamp-rs`](https://github.com/newrelic/newrelic-opamp-rs) (OpAMP client) and [`newrelic-auth-rs`](https://github.com/newrelic/newrelic-auth-rs) (`nr-auth`, OAuth2 token management) are git deps pinned to a tag, not crates.io releases.

## Changelog

When a change is user-facing, add a short, concise entry under `## Unreleased` in [`CHANGELOG.md`](CHANGELOG.md); its header explains the category keywords and what counts as user-facing.
