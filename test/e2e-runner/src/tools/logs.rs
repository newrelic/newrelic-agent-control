use std::fs;
use tracing::{error, info};

use crate::tools::test::TestResult;

/// Tool to show logs when a test is over
pub struct ShowLogsOnDrop<'a> {
    logs_path: &'a str,
}

impl<'a> From<&'a str> for ShowLogsOnDrop<'a> {
    fn from(value: &'a str) -> Self {
        Self { logs_path: value }
    }
}

impl<'a> Drop for ShowLogsOnDrop<'a> {
    fn drop(&mut self) {
        let _ = show_logs(self.logs_path);
    }
}

/// Shows logs from the specified path (supports glob patterns).
pub fn show_logs(logs_path: &str) -> TestResult<()> {
    info!("Showing Agent Control logs");
    let pattern = format!("{}*", logs_path);
    let paths = glob::glob(&pattern).map_err(|e| format!("failed to list log files: {}", e))?;

    for entry in paths {
        match entry {
            Ok(path) => {
                let content = fs::read_to_string(&path)
                    .map_err(|e| format!("could not read the log file in {:?}: {}", path, e))?;
                info!("Showing logs from '{}'", path.display());
                println!("---\n{content}\n---");
            }
            Err(e) => {
                error!("Error reading path: {}", e);
            }
        }
    }

    Ok(())
}
