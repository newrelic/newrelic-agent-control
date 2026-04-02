//! Platform-specific binary self-replacement logic.
//!
//! This crate provides a trait-based abstraction for replacing a running binary with a new version.
//! The implementation handles platform-specific details like atomic file operations, permission
//! preservation, and backup strategies.
//!

use std::path::Path;

/// Trait for platform-specific binary self-replacement.
pub trait SelfReplacer {
    type Error: std::error::Error;
    /// Replace the current binary with a new one.
    ///
    /// # Arguments
    ///
    /// * `new_bin` - Path to the new binary that will replace the current one
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the replacement was successful. After success, the caller
    /// should gracefully exit the process.
    fn self_replace(&self, new_bin: impl AsRef<Path>) -> Result<(), Self::Error>;
}
