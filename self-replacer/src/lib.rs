//! Platform-specific binary self-replacement logic.
//!

#![deny(missing_docs)]

use std::path::Path;

mod replacer;
pub use replacer::{BinarySelfReplacer, ReplaceError};

/// File extension appended to the original binary's name when a backup copy is created
/// (e.g. `agent-control.bak`). Backups are not removed automatically.
pub const BACKUP_SUFFIX: &str = "bak";

/// Trait for platform-agnostic binary self-replacement.
pub trait SelfReplacer {
    /// Error type returned when a replacement attempt fails.
    type Error: std::error::Error;

    /// Replaces the currently running binary with the binary at `new_bin`.
    fn self_replace(new_bin: impl AsRef<Path>) -> Result<(), Self::Error>;
}
