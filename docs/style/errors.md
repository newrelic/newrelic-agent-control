# Errors

## Error messages

Error messages **must begin with a lowercase letter and should not end with a period**.

Examples:

```rust
// 👍 Good:
#[derive(Debug, thiserror::Error)]
#[error("serializing string: {0}")]
pub struct Serialize(String);

// 👎 Bad:
// It must start with lowercase letter
#[derive(Debug, thiserror::Error)]
#[error("Serializing string: {0}")]
pub struct Serialize(String);
// It should not end with a period
#[derive(Debug, thiserror::Error)]
#[error("serializing string: {0}.")]
pub struct Serialize(String);
```

This format will result in nicer error message:

```bash
2025-08-19T16:57:39 ERROR serializing string: first token is invalid
```

as opposed to

```bash
2025-08-19T16:57:39 ERROR Serializing string: First token is invalid..
```

## thiserror `#[from]` attribute

When using thiserror, some team members advice against the use of the `#[from]` attribute.

```rust
// 👍 Good:
#[derive(Debug, thiserror::Error)]
#[error("serializing string: {0}")]
pub struct Serialize(#[from] SerializationError);
```

Using it, makes easier writing the code. We can call the `try-operator` (?) and the error is automatically transformed without any explicit type. However, this has a couple of disadvantages.

* It's difficult to know where an error comes from.
* It's difficult to know if an error is still used in the code.

We are not against the use of the `#[from]` attribute, but it's important to take the disadvantages into consideration before using it in a new error.
