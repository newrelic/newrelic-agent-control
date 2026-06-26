//! Filesystem helpers used across Agent Control.
//!
//! Provides small, mockable abstractions over the filesystem operations Agent Control
//! performs: reading, writing, renaming and deleting files ([`file`](mod@file)), creating and
//! removing directories ([`directory_manager`]), and path validation ([`utils`]). On
//! Windows it also exposes ACL helpers (`win_permissions`).

#![deny(missing_docs)]

/// Creating and removing directories on disk.
pub mod directory_manager;
/// Reading, writing, renaming and deleting files on disk.
pub mod file;
/// Path validation and the shared filesystem error type.
pub mod utils;
/// Windows-only helpers for restricting file permissions via ACLs.
#[cfg(target_family = "windows")]
pub mod win_permissions;

/// `mockall`-generated mocks of the file traits, enabled by the `mocks` feature.
#[cfg(feature = "mocks")]
#[allow(missing_docs)] // generated mock implementations
pub mod mock {
    use std::io;
    use std::path::Path;
    use std::path::PathBuf;

    use super::file::deleter::FileDeleter;
    use super::file::reader::FileReader;
    use super::file::renamer::FileRenamer;
    use super::file::writer::FileWriter;
    use mockall::mock;

    mock! {
        pub LocalFile {}

        impl FileReader for LocalFile {
            fn read(&self, file_path: &Path) -> io::Result<String>;
            fn dir_entries(&self, dir_path: &Path) -> io::Result<Vec<PathBuf>>;
        }

        impl FileRenamer for LocalFile {
            fn rename(&self, file_path: &Path, rename_path: &Path) -> io::Result<()>;
        }

        impl FileWriter for LocalFile {
            fn write(
                &self,
                path: &Path,
                buf: String,
            ) -> io::Result<()>;
        }

        impl FileDeleter for LocalFile {
            fn delete(&self, file_path: &Path) -> io::Result<()>;
        }
    }
}
