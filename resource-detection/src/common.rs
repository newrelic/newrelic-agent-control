//! Common structures for the crate.

/// Key represents the attribute name.
#[derive(Clone, Eq, Hash, PartialEq, Debug)]
pub struct Key(String);

/// Value represents the attribute value.
#[derive(Clone, Debug)]
pub struct Value(String);

impl From<String> for Key {
    fn from(string: String) -> Self {
        Self(string)
    }
}

impl From<&str> for Key {
    fn from(string: &str) -> Self {
        Self(string.to_string())
    }
}

impl From<String> for Value {
    fn from(string: String) -> Self {
        Self(string)
    }
}

impl From<Value> for String {
    fn from(val: Value) -> String {
        val.0
    }
}
