# Debugging

## Tokio console

The feature `tokio-console` enables [tokio-console](https://github.com/tokio-rs/console?tab=readme-ov-file#tokio-console)
which can be useful to debug tokio tasks.

1. Install the console:

```bash
$ cargo install --locked tokio-console
```

2. Compile and run the binary with the required flags and features:

* `RUSTFLAGS="--cfg tokio_unstable"` (tokio-console) requirement
* Set the `tokio-console` feature.

Example:

```bash
$ RUSTFLAGS="--cfg tokio_unstable" LOG_LEVEL="newrelic_agent_control=debug" cargo run --bin newrelic-agent-control --features k8s,tokio-console # ...
```

3. Execute tokio console

```bash
$ tokio-console
```
