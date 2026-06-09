# fake-opamp-server

A minimal, in-memory OpAMP server used to drive Agent Control in tests and demos. It speaks just enough of the [OpAMP protocol](https://github.com/open-telemetry/opamp-spec) to accept agent reports, send back remote configurations, and serve a JWKS document for signature verification.

It is **not** a production OpAMP server. The state is in-memory, the signing key is generated on startup, and the config hashing does not match Fleet Control's.

## Two ways to use it

### As a library (integration tests)

`FakeServer::start(handle)` spawns the HTTP server on a random port, on the provided tokio runtime. Tests then use methods like `set_config_response`, `get_health_status`, and `find_agent_control_instance` to drive and assert behavior. See `agent-control/tests/` for examples.

### As a standalone binary (manual testing / demos)

```sh
cargo run -p fake-opamp-server --bin fake-opamp-server                  # random free port
cargo run -p fake-opamp-server --bin fake-opamp-server 127.0.0.1:4320   # explicit address
```

On startup the binary prints the actual bound address and the URL of every endpoint:

```
fake-opamp-server listening on 127.0.0.1:4320

  OpAMP:        POST http://localhost:4320/opamp-fake-server
  JWKS:         GET  http://localhost:4320/jwks
  Admin state:  GET  http://localhost:4320/admin/state
  Admin config: POST http://localhost:4320/admin/agents/{instance_uid}/config

Press Ctrl+C to stop.
```

## Endpoints

| Method | Path                                           | Purpose                                                                  |
| ------ | ---------------------------------------------- | ------------------------------------------------------------------------ |
| `POST` | `/opamp-fake-server`                           | OpAMP `AgentToServer` ↔ `ServerToAgent` exchange (Protobuf body).         |
| `GET`  | `/jwks`                                        | JWKS document for the Ed25519 key used to sign remote-config messages.   |
| `GET`  | `/admin/state`                                 | Human-readable JSON snapshot of the in-memory `ServerState`.             |
| `POST` | `/admin/agents/{instance_uid}/config`          | Set the pending remote config for the given agent (overwrites previous). |

The admin endpoints are always registered, regardless of whether the server is started via the library or the binary.

## Driving the server with `curl`

Inspect the current state:

```sh
curl -s http://localhost:4320/admin/state | jq
```

Push a remote config from a YAML file to a specific agent. `instance_uid` is the agent's UUIDv7 in canonical (uppercase, no-hyphen) form — both hyphenated and unhyphenated forms are accepted by the path parameter:

```sh
jq -n --rawfile cfg ./agent-config.yaml '{agentConfig: $cfg}' \
  | curl -X POST http://localhost:4320/admin/agents/0190592A82877FB1A6D91ECAA57032BD/config \
      -H 'Content-Type: application/json' \
      --data-binary @-
```

The body is the raw `key -> yaml-body` map; multiple keys are allowed (use additional `--rawfile` arguments and extend the `jq` expression accordingly).

A successful POST returns `204 No Content`. An invalid `instance_uid` returns `400` with the parse error. A subsequent `GET /admin/state` will show the new config under the agent's `pending_remote_config` field — the server stops sending it once the agent acknowledges the hash.

## Caveats

- Fields in `GET /admin/state` that come from OpAMP proto types (see [newrelic/newrelic-opamp-rs](https://github.com/newrelic/newrelic-opamp-rs)) are rendered using their `Debug` impl. The JSON shape is meant for human inspection, not for programmatic consumption, it may change without notice.
- All state is lost when the process exits.
