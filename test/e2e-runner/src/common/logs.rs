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

/// Reads every log file under `logs_path` (matched as `{logs_path}*`) and returns `Ok(())` when
/// at least one line in one of those files contains every substring in `needles`. This is
/// stronger than "each needle appears somewhere in the log" — it proves the tokens co-occur on
/// the same log entry (e.g. an integration name together with a specific label value it carried).
pub fn expect_log_line_contains(logs_path: &str, needles: &[&str]) -> TestResult<()> {
    let pattern = format!("{}*", logs_path);
    let paths = glob::glob(&pattern).map_err(|e| format!("failed to list log files: {e}"))?;

    for entry in paths {
        let path = entry.map_err(|e| format!("error reading path: {e}"))?;
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("could not read log file {}: {e}", path.display()))?;
        for line in content.lines() {
            if needles.iter().all(|n| line.contains(n)) {
                return Ok(());
            }
        }
    }

    Err(format!("no log line under {logs_path}* contains all of {needles:?}").into())
}
