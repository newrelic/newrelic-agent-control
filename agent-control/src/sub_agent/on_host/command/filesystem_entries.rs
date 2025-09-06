use std::{collections::HashMap, fs, io, path::PathBuf};

use crate::agent_type::runtime_config::on_host::filesystem::FileSystem;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderedFileSystemEntries(HashMap<PathBuf, String>);

impl From<FileSystem> for RenderedFileSystemEntries {
    fn from(value: FileSystem) -> Self {
        Self(value.rendered())
    }
}

impl RenderedFileSystemEntries {
    pub fn write(&self) -> io::Result<()> {
        self.0.iter().try_for_each(|(path, content)| {
            let parent_dir = path.parent().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidFilename,
                    format!("{} has no parent dir", path.display()),
                )
            })?;
            fs::create_dir_all(parent_dir)?;
            // Will overwrite files if they already exist!
            fs::write(path, content)
        })
    }
}
