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

```rust
// 👍 Good:
operation.map_err(|err| OperationError::Generic(format!("operation failed: {err}")))?;

// 👎 Bad:
// It must start with lowercase letter
operation.map_err(|err| OperationError::Generic(format!("Operation failed: {err}")))?;
// It should not end with a period
operation.map_err(|err| OperationError::Generic(format!("operation failed: {err}.")))?;
```

This format will result in nicer error message:

```bash
2025-08-19T16:57:39 ERROR serializing string: first token is invalid
```

as opposed to

```bash
2025-08-19T16:57:39 ERROR Serializing string: First token is invalid..
```

## Tips for building new errors

We use `thiserror` to build new error types. It's a powerful tool. However, it can introduce some complexity to the codebase if taken lightly.
The main issue we found with it is that we might end up with an error type enum that contains dozens of variants that we don't leverage. It might also happen that one of the variants is `Generic`. Which is odd. Either we are missing errors or we don't need that variant.

There are use cases for using enums with different variants:

* when we want to match a specific "sub-error" type in a test
* avoid duplication of error messages
* controlling the flow of the program

We have a rule to help us avoiding the issues previously mentioned. Create a simple error type as struct. Then, if we need to match a specific error or avoid error message duplication, we can think of "promoting" the struct to an enum.

```rust
#[derive(Debug, Error)]
#[error("resolving k8s secret: {0}")]
pub struct K8sSecretProviderError(String);
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
