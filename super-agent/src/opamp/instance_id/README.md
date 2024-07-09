# InstanceID

InstanceID represents
an [Agent Instance ID](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#agenttoserverinstance_uid)

Currently, it's represented by
a [UUID v7](https://www.ietf.org/archive/id/draft-ietf-uuidrev-rfc4122bis-14.html#name-uuid-version-7)

For a matter of simplicity in the API it has been agreed with the Fleet Management Team
that the String representation of an InstanceID will be uppercase and without hyphens

```
0190592a-8287-7fb1-a6d9-1ecaa57032bd
```

Will be represented as:

```
0190592A82877FB1A6D91ECAA57032BD
```

For the communication we will use the Bytes format.

The used crate ([uuid](https://github.com/uuid-rs/uuid/blob/1.10.0/src/fmt.rs#L72)) already supports this format:

```rust
/// Format a [`Uuid`] as a hyphenated string, like
/// `67e55044-10b1-426f-9247-bb680e5fe0c8`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Hyphenated(Uuid);

/// Format a [`Uuid`] as a simple string, like
/// `67e5504410b1426f9247bb680e5fe0c8`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Simple(Uuid);
```
