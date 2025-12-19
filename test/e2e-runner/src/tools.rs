use std::fs;

use tracing::warn;

use crate::tools::test::TestResult;

pub mod config;
pub mod logs;
pub mod nrql;
pub mod test;

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
