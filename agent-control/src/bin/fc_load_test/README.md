# Fleet Control load-test simulator

Runs many on-host Agent Controls in one process to load-test the Fleet Control (FC) server.
Each simulated Agent Control is the real `AgentControl` (real OpAMP polls, real remote config
handling) but looks like a separate host and never starts processes or downloads packages.
Not shipped and not meant to be merged.

Every change to existing library code is marked with a `load-test` comment:

```sh
git grep -n "load-test"
```

## What each simulated Agent Control does differently

- Hostname, host id and machine id are `<run_prefix>-<index:04>`; no host or cloud detection.
- One cached token is shared by every OpAMP client in the process; each simulated host has its
  own HTTP client (shared by its Agent Control and sub-agents), so FC sees one set of
  connections per simulated host, as with real hosts. All of them come from this machine's IP.
- One tokio runtime and one OCI client are shared; the agent type registry probe runs once.
- Every embedded agent type is shadowed by a no-op one (no executables, no packages) with a
  `File` health check reading `<data_root>/healthy.yaml`, so sub-agents report healthy.
- `self_update` and the status `server` are forced off; `oci.registry` points to an
  unreachable address so an unexpected agent type fails fast instead of being downloaded.

## Run

```sh
cargo build --release --bin newrelic-agent-control-fc-load-test
./target/release/newrelic-agent-control-fc-load-test fc-load-test.yaml
```

Start from `agent-control/fc-load-test.example.yaml`. The `agent_control` block is a regular
on-host Agent Control config (prod or staging endpoints, fleet id, system identity). Ctrl-C
stops every simulated Agent Control gracefully.

Sub-agents come from FC: deploy a config to the fleet listing the agents (any embedded agent
type id works, it is a no-op here), then a config for each sub-agent. A sub-agent starts
reporting health only once it has received its own config.

## Identities

Data lives in `<data_root>/<run_prefix>-<index:04>/`.

- Same `run_prefix`, data kept: the same instances (and their last remote config) come back.
- New `run_prefix` or `fresh: true`: brand-new instances (`fresh` deletes only the
  directories of this process's indices, `index_offset..index_offset+count`).

## Scale on macOS

macOS allows 16,384 threads per process (`sysctl kern.num_taskthreads`). Going over it makes
a library thread spawn panic and **aborts the whole process**. Expect ~18 threads per simulated
Agent Control with 4 configured sub-agents (the simulator warns at startup when the estimate
is close to the limit), so on macOS run the 1k target as two processes:

```yaml
# process A                  # process B
count: 500                   count: 500
index_offset: 0              index_offset: 500
```

Same `run_prefix` and `data_root` for both. `fresh: true` only wipes the indices of the
process that sets it. Measured locally (debug build, fake server): 1,000 Agent Controls without
sub-agents use ~4,400 threads and ~760 MB RSS. The ramp-up log line reports the real count.

The health check hook also applies to the regular binaries: the health checker now builds its
HTTP client only when an `Http` check is configured.

## Local smoke test

```sh
cargo run -p fake-opamp-server -- 127.0.0.1:4318
```

Use `endpoint: http://localhost:4318/opamp-fake-server`, no `auth_config` and
`signature_validation.enabled: false`. Inspect with `curl localhost:4318/admin/state` and push
a config with `POST /admin/agents/<instance_uid>/config` and a body like
`{"agentConfig": "<yaml>"}`.
