use std::fs;
use tracing::{debug, error, info};

use crate::common::test::TestResult;

/// Shows logs from the specified path (supports glob patterns).
pub fn show_logs(logs_path: &str) -> TestResult<()> {
    info!("Showing Agent Control logs");

    let pattern = format!("{}*", logs_path);
    debug!("Listing log files with pattern: {pattern}");

    let paths = glob::glob(&pattern).map_err(|e| format!("failed to list log files: {e}"))?;
    let paths: Vec<_> = paths.collect();
    debug!("Found log file entries: {paths:?}");

    for entry in paths {
        debug!("Processing log file entry {entry:?}");
        match entry {
            Ok(path) => {
                let path_display = path.display();
                let content = fs::read_to_string(&path)
                    .map_err(|e| format!("could not read the log file in {path_display}: {e}"))?;
                info!("Showing logs from '{path_display}'");
                println!("---\n{content}\n---");
            }
            Err(e) => error!("Error reading path: {e}"),
        }
    }

    Ok(())
}
