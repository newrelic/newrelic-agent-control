use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};

use crate::opamp::instance_id::getter::DataStored;

#[derive(Default)]
pub struct FileReader;

#[derive(thiserror::Error, Debug)]
pub(super) enum FileReaderError {
    #[error("Path {0} does not exist")]
    PathDoesNotExist(PathBuf),
    #[error("I/O error: `{0}`")]
    IOError(#[from] io::Error),
    #[error("Error deserializing into a DataStored file:`{0}`")]
    Deserialization(#[from] serde_yaml::Error),
}

#[cfg_attr(test, mockall::automock)]
impl FileReader {
    pub(super) fn read(&self, path: &Path) -> Result<DataStored, FileReaderError> {
        if !path.exists() {
            return Err(FileReaderError::PathDoesNotExist(path.to_path_buf()));
        }
        Ok(serde_yaml::from_reader(File::open(path)?)?)
    }
}
