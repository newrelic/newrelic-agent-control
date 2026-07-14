# oci-test-utils

In-tree test utilities for pushing **agent packages** and **agent type definition** artifacts to an OCI registry — local (e.g. a `zot`/`registry:2` on `localhost:5001`) or remote (Docker Hub, GHCR).

It is **not** production tooling. Distribution is source-only (the crate has `publish = false`); there are no published binaries. Run via `cargo run` or vendor the library into integration tests.

## Two ways to use it

### As a library (integration tests)

`PackagePublisher` is the workhorse. Tests construct one and push artifacts as part of fixture setup:

```rust
use oci_test_utils::{LOCAL_HTTP_REGISTRY_URL, PackageMediaType, PackagePublisher};

let publisher = PackagePublisher::new(runtime.handle().clone(), LOCAL_HTTP_REGISTRY_URL);
let reference = publisher.push_with_tag(
    &Path::new("./fixtures/agent.tar.gz"),
    PackageMediaType::TarGz,
    "1.2.3",
);
```

The library exposes:

- `PackagePublisher` — publishes both agent packages and agent type artifacts. Supports an optional repository name (default `"test"`) and either basic or bearer-token auth.
- `AgentTypeDefinitionMeta` — minimal agent type metadata + the `<environment-prefix>-<name>-<version>` tag composition rule mirrored from `agent-control/src/agent_type/oci.rs`.
- `OCISigner` — generates ephemeral signing keys and produces cosign-style signatures (used by tests exercising AC's signature-verification path).

### As a standalone binary (`oci-utils`)

```sh
# Pre-built package, explicit tag
cargo run -p oci-test-utils --bin oci-utils -- \
    push-package --tag 1.2.3 ./newrelic-infra.tar.gz

# Agent type — tag and archive are derived from the YAML metadata
cargo run -p oci-test-utils --bin oci-utils -- \
    push-agent-type ./agent-type.yaml
```

On success each subcommand prints the resulting reference and digest:

```
pushed agent type
  reference: localhost:5001/test:kubernetes-some.agent.type-0.0.123
  digest:    sha256:…
```

## Global options

| Flag | Default | Description |
| --- | --- | --- |
| `--registry <URL>` | `localhost:5001` | Registry host. Plain HTTP only for `localhost:5001` (mirrors `HttpsExcept`); everything else uses HTTPS. |
| `--repository <NAME>` | `test` | Repository/namespace within the registry (e.g. `myorg/newrelic-agent`). |
| `--username <USER>` | — | Basic-auth username. |
| `--password <PASS>` | — | Basic-auth password. Prefer `--password-stdin` or `OCI_UTILS_PASSWORD`. |
| `--password-stdin` | — | Read the basic-auth password from stdin. |
| `--token <TOKEN>` | — | Bearer-token auth. Mutually exclusive with `--username`/`--password`. Prefer `--token-stdin` or `OCI_UTILS_TOKEN`. |
| `--token-stdin` | — | Read the bearer token from stdin. |

Auth precedence: bearer token if provided, otherwise basic auth if a username is provided, otherwise anonymous.

## Subcommands

### `push-package`

```
push-package [--media-type tar-gz|zip] --tag <TAG> <FILE>
```

| Arg / flag | Description |
| --- | --- |
| `<FILE>` | Pre-built package archive to upload. |
| `--media-type` | `tar-gz` (default) or `zip`. |
| `--tag` | Tag to push under. Idempotent: same file + tag → same content digest. |

### `push-agent-type`

```
push-agent-type <DEFINITION.yaml>
```

| Arg | Description |
| --- | --- |
| `<DEFINITION.yaml>` | The agent type definition. Both the OCI tag and the archive layout are derived from this file. |

The CLI:

1. Reads the YAML and parses the minimal metadata (`namespace`, `name`, `version`, `platform`, `operating_system`).
2. Derives the OCI tag matching what Agent Control pulls: `<environment-prefix>-<name>-<version>`, where the prefix is `host-linux`, `host-windows`, or `kubernetes` depending on the platform/OS pair.
3. Builds a `tar.gz` containing the definition named `<tag>.yaml` (the layout the downloader expects).
4. Pushes the single derived tag.

There is no `--tag`, `--media-type`, or `--environment` here — the tag and environment are metadata-derived and agent types are always `tar+gzip`.

## End-to-end example

```sh
# 1. Local registry
docker run -d -p 5001:5000 --name oci registry:2

# 2. Push a pre-built package (explicit tag) and an agent type (tag derived from the YAML)
cargo run -p oci-test-utils --bin oci-utils -- \
    push-package --tag 1.2.3 ./newrelic-infra.tar.gz
cargo run -p oci-test-utils --bin oci-utils -- \
    push-agent-type ./agent-type.yaml

# 3. Point Agent Control at localhost:5001 and onboard.
```

Remote registry with basic auth:

```sh
echo "$REGISTRY_PASSWORD" | cargo run -p oci-test-utils --bin oci-utils -- \
    --registry ghcr.io --repository myorg/nr-agent \
    --username myorg --password-stdin \
    push-package --tag 1.2.3 ./agent.tar.gz
```

Remote registry with bearer-token auth:

```sh
echo "$GHCR_TOKEN" | cargo run -p oci-test-utils --bin oci-utils -- \
    --registry ghcr.io --repository myorg/nr-agent \
    --token-stdin \
    push-agent-type ./agent-type.yaml
```

## Notes for maintainers

- `agent_type_meta.rs` is a deliberate, hand-maintained mirror of `agent-control/src/agent_type/oci.rs::AgentTypeTag` and its environment-prefix helper. The duplication is intentional: this crate cannot depend on `newrelic-agent-control`. Keep the two in sync by hand when either side changes.
- `flate2` and `tar` are pinned to the same versions agent-control uses so the workspace resolves to one copy each.
