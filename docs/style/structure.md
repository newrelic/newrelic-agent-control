# Structure

## Abstractions

Only the minimum required abstractions should be created.

- Abstractions **MUST** be created when multiple implementations exist.
- Abstractions **SHOULD** be created because they make sense, not because of convenience.
- Abstractions **CAN** be created to allow mocks in testing.
- Abstractions should not be coupled to underlying implementations. The error returned by a trait method should not know about specific implementations.

## Constructors

Constructors should not execute actions beyond validations.

```rust
// 👎 Bad:
// Constructors should not trigger side effects
impl Agent {
    fn default() -> Self {
        self.load_agent_cfgs(); // side effect in constructor
        ...
    }
}
```

- Use `Default` trait for constructors **without** parameters.
- Use `new` function name for constructors **with** parameters.

### Nullary constructors vs `Default`

`Default::default()` should be deterministic — two calls must create **exactly the same value**.

A `pub fn new() -> T` can be added for types where this is not the case (e.g. types containing a `SystemTime`, a key pair, or similar non-deterministic values). To avoid Clippy warnings in these situations, use a name other than `new`, such as `create`, `generate`, etc.

## `From`/`Into` Traits

Use Rust's `From` and `Into` traits for conversion and transformation:

```rust
// 👍 Good: converting between error types or equivalent structs
impl From<SerializationError> for MyError { ... }
impl From<WireConfig> for InternalConfig { ... }

// 👎 Bad: using From for creation or as a getter
impl From<&ProcessRunner<Started>> for Metadata {
    fn from(value: &ProcessRunner<Started>) -> Self {
        value.metadata.clone() // this is a getter, not a conversion
    }
}
```

- Use for: error-to-error conversion, struct-to-equivalent-struct conversion.
- Do not use for: object creation, getters.

## Getter Signature

When a fetched value might not exist, use this signature:

```rust
fn get() -> Result<Option<Value>, Error>
```

Return `None` to denote the absence of the value. Only deviate when the absence must trigger a side effect (e.g. creating a new instance ID if one does not exist).

## Visibility

- General rule: expose as little as necessary.
- Struct fields should be private; expose only public methods.

## Size

Functions and methods should be small. If a method has many responsibilities, it should delegate to other methods:

```rust
// 👍 Good:
fn run(&self) {
    self.run_opamp_client();
    self.loop_event();
    self.shutdown();
}

// 👎 Bad:
fn run(&self) {
    // ... create OpAMP client inline ...
    // ... loop event inline ...
    // ... handle shutdown inline ...
}
```

## Serialization / Intermediate Structs

Intermediate structs used only for deserialization should be hidden inside the service/module that uses them (not exposed publicly).

```rust
// 👍 Good: helper is private inside the deserializer module
struct EndSpecDeserializeHelper { ... }

// 👎 Bad: helper is a top-level public type
pub struct IntermediateEndSpec { ... }
```
