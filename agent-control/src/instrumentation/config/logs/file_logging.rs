use super::config::LoggingConfigError;
use crate::agent_control::defaults::{AGENT_CONTROL_ID, AGENT_CONTROL_LOG_FILENAME};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};

#[derive(Debug, Deserialize, Default, PartialEq, Clone)]
pub(crate) struct FileLoggingConfig {
    pub(crate) enabled: bool,
    // Default value is being set by `ConfigPatcher` right after deserialization.
    pub(crate) path: Option<LogFilePath>,
}

impl FileLoggingConfig {
    pub(crate) fn setup(
        self,
        default_dir: PathBuf,
    ) -> Result<Option<(NonBlocking, WorkerGuard)>, LoggingConfigError> {
        if !self.enabled {
            return Ok(None);
        }

        // if path is not specified into the config we fall back to a default path
        let log_file = self.path.unwrap_or(LogFilePath::new(&default_dir));

        let file_appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_suffix(log_file.file_name())
            .build(log_file.parent)
            .map_err(|e| LoggingConfigError::FileLoggingConfig(e.to_string()))?;

        Ok(Some(tracing_appender::non_blocking(file_appender)))
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
#[serde(try_from = "PathBuf")]
pub(crate) struct LogFilePath {
    parent: PathBuf,
    file_name: PathBuf,
}

impl LogFilePath {
    pub fn new(base_dir: &Path) -> Self {
        Self {
            parent: base_dir.join(AGENT_CONTROL_ID),
            file_name: PathBuf::from(AGENT_CONTROL_LOG_FILENAME),
        }
    }

    pub fn file_name(&self) -> String {
        self.file_name
            .to_str()
            .expect("file name should be a valid UTF-8 string")
            .to_string()
    }
}

impl TryFrom<PathBuf> for LogFilePath {
    type Error = LoggingConfigError;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        value.to_str().ok_or(LoggingConfigError::InvalidFilePath(
            "file path provided should be a valid UTF-8 string".into(),
        ))?;

        let parent = value
            .parent()
            .ok_or(LoggingConfigError::InvalidFilePath(
                "file path provided must have a valid parent directory".into(),
            ))?
            .into();
        let file_name = value
            .file_name()
            .ok_or(LoggingConfigError::InvalidFilePath(
                "file path provided must have a valid file name".into(),
            ))?
            .into();
        Ok(Self { parent, file_name })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[cfg(unix)]
    fn non_utf8_path() -> PathBuf {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(OsString::from_vec(vec![0xFF, 0xFE, 0x00]))
    }

    #[cfg(windows)]
    fn non_utf8_path() -> PathBuf {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        // Unpaired surrogate — invalid UTF-16, so to_str() returns None
        PathBuf::from(OsString::from_wide(&[0xD800]))
    }

    #[rstest]
    #[case(PathBuf::from("/some/.."), "file name")]
    #[case(PathBuf::from("/"), "parent directory")]
    #[case(non_utf8_path(), "valid UTF-8")]
    fn test_log_file_path_try_from_errors(#[case] input: PathBuf, #[case] expected_msg: &str) {
        let err = LogFilePath::try_from(input).expect_err("expected an error");
        assert!(
            err.to_string().contains(expected_msg),
            "error '{}' should contain '{}'",
            err,
            expected_msg
        );
    }
}
