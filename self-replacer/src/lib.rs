//! Platform-specific binary self-replacement logic.
//!

use std::path::Path;

#[cfg(target_os = "windows")]
pub mod windows;

/// Trait for platform-specific binary self-replacement.
pub trait SelfReplacer {
    type Error: std::error::Error;

    /// Replaces the currently running binary with the binary at `new_bin`.
    fn self_replace(&self, new_bin: impl AsRef<Path>) -> Result<(), Self::Error>;
}
