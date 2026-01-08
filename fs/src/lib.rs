pub mod directory_manager;
pub mod file;
pub mod utils;
#[cfg(target_family = "windows")]
pub mod win_permissions;

#[cfg(feature = "mocks")]
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
