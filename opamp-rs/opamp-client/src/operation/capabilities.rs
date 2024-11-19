//! Provides an abstraction over the OpAMP AgentCapabilities protobuffer definition.

use crate::opamp::proto::AgentCapabilities;

/// A set of capabilities represented as bit flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities(i32);

impl Capabilities {
    /// Creates a new `Capabilities` instance from a vector of `AgentCapabilities`.
    ///
    /// # Arguments
    ///
    /// * `caps` - A vector of `AgentCapabilities` to be combined into the `Capabilities` instance.
    ///
    /// # Example
    ///
    /// ```
    /// use opamp_client::operation::capabilities::Capabilities;
    /// use opamp_client::opamp::proto::AgentCapabilities;
    ///
    /// let caps = Capabilities::new(vec![AgentCapabilities::AcceptsRestartCommand]);
    /// ```
    pub fn new(caps: Vec<AgentCapabilities>) -> Self {
        Self(caps.into_iter().fold(0i32, |c1, c2| c1 | c2 as i32))
    }

    /// Checks if the `Capabilities` instance has a specific capability.
    ///
    /// # Arguments
    ///
    /// * `capability` - The `AgentCapabilities` to check for.
    ///
    /// # Returns
    ///
    /// * `true` if the capability is present in the `Capabilities`, otherwise `false`.
    ///
    /// # Example
    ///
    /// ```
    /// use opamp_client::operation::capabilities::Capabilities;
    /// use opamp_client::opamp::proto::AgentCapabilities;
    ///
    /// let caps = Capabilities::new(vec![AgentCapabilities::AcceptsRestartCommand]);
    /// assert!(caps.has_capability(AgentCapabilities::AcceptsRestartCommand));
    /// assert!(!caps.has_capability(AgentCapabilities::AcceptsPackages));
    /// ```
    pub fn has_capability(self, capability: AgentCapabilities) -> bool {
        self.0 & capability as i32 != 0
    }
}

/// A macro for creating a `Capabilities` instance with multiple capabilities.
///
/// This macro allows you to create a `Capabilities` instance with multiple
/// `AgentCapabilities` in a concise manner.
///
/// # Example
///
/// ```
/// use opamp_client::{capabilities, operation::capabilities::{Capabilities}};
/// use opamp_client::opamp::proto::AgentCapabilities;
///
/// let caps = capabilities!(
///     AgentCapabilities::AcceptsRestartCommand,
///     AgentCapabilities::AcceptsPackages,
///     AgentCapabilities::AcceptsRemoteConfig
/// );
/// ```
#[macro_export]
macro_rules! capabilities {
    ($($cap:expr),*) => {{
        use $crate::operation::capabilities::Capabilities;
        let caps: Vec<AgentCapabilities> = vec![AgentCapabilities::Unspecified $(, $cap)*];
        Capabilities::new(caps)
    }};
}

impl From<Capabilities> for u64 {
    fn from(value: Capabilities) -> Self {
        value.0 as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentCapabilities::*;

    #[test]
    fn test_many_capabilities() {
        let caps = capabilities!(AcceptsRestartCommand, AcceptsPackages, AcceptsRemoteConfig);
        assert!(caps.has_capability(AcceptsRestartCommand));
        assert!(caps.has_capability(AcceptsPackages));
        assert!(caps.has_capability(AcceptsRemoteConfig));

        assert!(!caps.has_capability(AcceptsOpAmpConnectionSettings));
    }

    #[test]
    fn test_single_capability() {
        let caps = capabilities!(AcceptsRestartCommand);
        assert!(caps.has_capability(AcceptsRestartCommand));
        assert!(!caps.has_capability(AcceptsPackages));
        assert!(!caps.has_capability(AcceptsRemoteConfig));
    }

    #[test]
    fn test_no_capabilities() {
        let caps = capabilities!();
        assert!(!caps.has_capability(AcceptsRestartCommand));
        assert!(!caps.has_capability(AcceptsOpAmpConnectionSettings));

        assert_eq!(caps, Capabilities::default());
        assert_eq!(caps, Capabilities(0));
    }
}
