use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use crate::agent_type::runtime_config::on_host::filesystem::FileSystem;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderedFileSystemEntries(HashMap<PathBuf, String>);

impl RenderedFileSystemEntries {
    pub fn from_agent_type_filesystem(
        base_dir: impl AsRef<Path>,
        file_entries: FileSystem,
    ) -> Self {
        let rendered_entries = file_entries.rendered();

        // we know that the paths in `rendered_entries` are relative and do not go above their
        // base dir (e.g. `/../../`) due to the parse-time validations of [`FileSystem`], so here
        // we "safely" prepend the provided base dir to them.
        let entries = rendered_entries
            .into_iter()
            .map(|(path, content)| {
                let full_path = base_dir.as_ref().join(path);
                (full_path, content)
            })
            .collect();

        Self(entries)
    }

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
