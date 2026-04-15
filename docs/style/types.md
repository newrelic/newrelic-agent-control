# Types

## Encoding state in types

Prefer different types over different values of the same type for representing state:

```rust
// 👍 Good: separate types for each state
struct Stopped;
struct Running;

// 👎 Bad: bool field to represent binary state
struct Agent {
    running: bool,
}
```

Do not use a `bool` field inside a struct to represent binary state. If an enum is truly needed, it is almost always better than a `bool` because it can be extended and both the type and its variants can have explanatory names.

## Type-state pattern

When two or more types participate in a state transition and share many fields or behavior, use the [type-state pattern](https://zerotomastery.io/blog/rust-typestate-patterns/) with generics:

```rust
struct Stopped;
struct Running;
struct Finished {
    result_message: String,
}

pub struct Transition<T> {
    name: String,
    modified_timestamp: u64,
    state: T,
}

impl<T> Transition<T> {
    fn with_modification_time(self) -> Self { /* ... */ }
}

impl Transition<Stopped> {
    pub fn run(self) -> Transition<Running> { /* ... */ }
}

impl Transition<Running> {
    pub fn wait(self) -> Transition<Finished> { /* ... */ }
}

impl Transition<Finished> {
    pub fn get_result_msg(&self) -> &str {
        &self.state.result_message
    }
}
```

This pattern gives compile-time guarantees: calling `run` twice, or calling `wait` before `run`, is a compiler error.

**Use this pattern only when:**

- Each parameterized type uses the same shared fields frequently.
- The state type parameters (`T`s) are minimal — ideally empty marker types.
- When state types carry no data, use `PhantomData` instead of a field.

**Restrict valid type parameters** using trait bounds or sealed traits when exposing types with this pattern publicly.

> This pattern has an impact on readability — do not use it lightly.

## Enums

Use `enum`s for types whose values can have very different data but ultimately represent the same concept, and where all variants may be checked and operated on differently:

```rust
// 👍 Good: events from different domains are separate types
enum SubAgentEvent { ... }
enum OpAMPEvent { ... }
```

- Errors and events that operate on different domains must be different types.
- Avoid enums with too many variants.
- Each variant should have a distinct `Display` implementation so its origin can be located in the code.
