# Testing

## Test module

Test-only code must live under `mod tests`, not in production code paths. Do not mix test and production code:

```rust
// 👎 Bad: test constructor mixed into production impl block
impl Resolver {
    #[cfg(test)]
    fn new<T>(source: T) -> Self { ... }

    fn build_config(self) -> Result<SuperAgentConfig, SuperAgentConfigError> { ... }
}

// 👍 Good: test constructor is inside the test module
#[cfg(test)]
mod tests {
    impl Resolver {
        fn new<T>(source: T) -> Self { ... }
    }

    #[test]
    fn test_something() { ... }
}
```

## Assertions

Tests must assert on values, not only on the absence of errors:

```rust
// 👎 Bad: only asserts the operation didn't panic
let config = parse_config(input);
assert!(config.is_ok());

// 👍 Good: asserts the parsed values are correct
let config = parse_config(input).unwrap();
assert_eq!(config.name, "expected-name");
assert_eq!(config.timeout_secs, 30);
```

## `assert_eq!` argument order

Rust's standard library convention is `assert_eq!(actual, expected)` — natural language order: "assert that `<actual>` equals `<expected>`":

```rust
// 👍 Good: actual first, expected second
assert_eq!(config.name, "expected-name");

// 👎 Bad: expected first (xUnit convention — not idiomatic Rust)
assert_eq!("expected-name", config.name);
```

## Test helpers

When multiple tests repeat the same setup (e.g. creating a temp dir and fixed file paths), extract it into a small struct rather than duplicating the lines in every test. Keep the helper minimal — just enough to eliminate the repetition without hiding what each test is actually doing.

## Parameterized tests with `rstest`

When multiple test cases exercise the same logic with different inputs or expected outputs, use [`rstest`](https://docs.rs/rstest) instead of duplicating test functions:

```rust
use rstest::rstest;

#[rstest]
#[case("applying", ConfigState::Applying)]
#[case("failed",   ConfigState::Failed)]
fn parses_config_state(#[case] input: &str, #[case] expected: ConfigState) {
    let state: ConfigState = input.parse().unwrap();
    assert_eq!(state, expected);
}
```

Use named cases (`#[case::name(...)]`) when the inputs alone do not make the intent clear:

```rust
#[rstest]
#[case::single_layer(vec!["application/vnd.newrelic.agent.layer.tar+gzip"])]
#[case::extra_layers(vec!["application/vnd.newrelic.agent.layer.tar+gzip", "application/vnd.custom.extra.v1"])]
fn accepts_valid_manifest(#[case] layer_media_types: Vec<&str>) {
    // ...
}
```

Do not use `rstest` for a single case or when the implemented logic has complex conditionals based on test parameters — write a plain `#[test]` instead.
