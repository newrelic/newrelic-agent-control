use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use tracing::warn;

use crate::common::test::TestResult;

/// Writes contents to a file and ensures data is flushed to disk before returning.
/// If the file does not exist, it will be created. If it does exist, it will be truncated.
pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) {
    let mut file_options = OpenOptions::new();
    file_options.write(true).create(true).truncate(true);

    let mut file = file_options.open(path.as_ref()).unwrap_or_else(|err| {
        panic!(
            "Could not open file for writing: {}: {}",
            path.as_ref().display(),
            err
        )
    });
    file.write_all(contents.as_ref()).unwrap_or_else(|err| {
        panic!(
            "Could not write to file: {}: {}",
            path.as_ref().display(),
            err
        )
    });
    file.sync_data().unwrap_or_else(|err| {
        panic!(
            "Could not sync data to disk for file: {}: {}",
            path.as_ref().display(),
            err
        )
    });
}

/// Removes the directories receives as list
pub fn remove_dirs(dirs: &[&str]) -> TestResult<()> {
    for dir in dirs {
        match fs::remove_dir_all(dir) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                warn!(directory = dir, "Directory not found");
            }
            Err(e) => {
                return Err(format!("could not remove {:?}: {}", dir, e).into());
            }
        }
    }
    Ok(())
}
