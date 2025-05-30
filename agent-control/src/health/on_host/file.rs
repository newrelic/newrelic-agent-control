use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use crate::health::health_checker::{HealthChecker, HealthCheckerError, Healthy, Unhealthy};
use crate::health::with_start_time::HealthWithStartTime;
use crate::utils::time::sys_time_from_unix_timestamp;

pub struct FileHealthChecker {
    file_path: PathBuf,
}

impl FileHealthChecker {
    pub fn new(file_path: PathBuf) -> Self {
        Self { file_path }
    }
}

impl HealthChecker for FileHealthChecker {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        let file_content = fs::read(&self.file_path).map_err(|e| {
            HealthCheckerError::Generic(format!(
                "reading health file '{}': {}",
                self.file_path.display(),
                e
            ))
        })?;

        let health: FileHealthContent = serde_yaml::from_slice(&file_content).map_err(|e| {
            HealthCheckerError::Generic(format!(
                "parsing health file '{}': {}",
                self.file_path.display(),
                e
            ))
        })?;

        Ok(health.into())
    }
}

#[derive(Debug, Deserialize)]
struct FileHealthContent {
    healthy: bool,
    status: String,
    #[serde(default)]
    last_error: String,
    start_time_unix_nano: u64,
    status_time_unix_nano: u64,
}

impl From<FileHealthContent> for HealthWithStartTime {
    fn from(content: FileHealthContent) -> Self {
        let status_time = sys_time_from_unix_timestamp(content.status_time_unix_nano);
        let start_time = sys_time_from_unix_timestamp(content.start_time_unix_nano);

        if content.healthy {
            HealthWithStartTime::from_healthy(
                Healthy::new(content.status).with_status_time(status_time),
                start_time,
            )
        } else {
            HealthWithStartTime::from_unhealthy(
                Unhealthy::new(content.status, content.last_error).with_status_time(status_time),
                start_time,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;
    use std::time::{Duration, UNIX_EPOCH};
    use tempfile::TempDir;

    use crate::health::health_checker::{HealthChecker, Healthy, Unhealthy};
    use crate::health::with_start_time::HealthWithStartTime;

    use super::FileHealthChecker;

    #[test]
    fn successful_cases() {
        struct TestCase {
            name: &'static str,
            file_content: &'static str,
            expected_health: HealthWithStartTime,
        }
        impl TestCase {
            fn run(&self) {
                let tmp_dir = TempDir::new().unwrap();
                let mut file = File::create_new(tmp_dir.path().join("test")).unwrap();
                file.write_all(self.file_content.as_bytes()).unwrap();

                let health_checker = FileHealthChecker {
                    file_path: tmp_dir.path().join("test"),
                };

                let h = health_checker.check_health().unwrap();

                assert_eq!(h, self.expected_health, "test case: {}", self.name);
                assert_eq!(
                    h.status_time(),
                    self.expected_health.status_time(),
                    "test case: {}",
                    self.name
                );
            }
        }

        let test_cases = vec![
            TestCase {
                name: "healthy",
                file_content: r#"
healthy: true
status: "some agent-specific message"
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444001               
"#,
                expected_health: HealthWithStartTime::from_healthy(
                    Healthy::new("some agent-specific message".into())
                        .with_status_time(UNIX_EPOCH + Duration::from_nanos(1725444001)),
                    UNIX_EPOCH + Duration::from_nanos(1725444000),
                ),
            },
            TestCase {
                name: "healthy with last_error",
                file_content: r#"
healthy: true
status: "some agent-specific message"
last_error: "this should be ignored"
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444001               
"#,
                expected_health: HealthWithStartTime::from_healthy(
                    Healthy::new("some agent-specific message".into())
                        .with_status_time(UNIX_EPOCH + Duration::from_nanos(1725444001)),
                    UNIX_EPOCH + Duration::from_nanos(1725444000),
                ),
            },
            TestCase {
                name: "unhealthy",
                file_content: r#"
healthy: false
status: "some agent-specific message"
last_error: "some error message"
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444001               
"#,
                expected_health: HealthWithStartTime::from_unhealthy(
                    Unhealthy::new(
                        "some agent-specific message".into(),
                        "some error message".into(),
                    )
                    .with_status_time(UNIX_EPOCH + Duration::from_nanos(1725444001)),
                    UNIX_EPOCH + Duration::from_nanos(1725444000),
                ),
            },
            TestCase {
                name: "unhealthy optional last_error",
                file_content: r#"
healthy: false
status: "some agent-specific message"
# last_error: "last_error is optional"
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444001               
"#,
                expected_health: HealthWithStartTime::from_unhealthy(
                    Unhealthy::new("some agent-specific message".into(), "".into())
                        .with_status_time(UNIX_EPOCH + Duration::from_nanos(1725444001)),
                    UNIX_EPOCH + Duration::from_nanos(1725444000),
                ),
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    #[test]
    fn fail_when_fail_to_read_file() {
        let tmp_dir = TempDir::new().unwrap();

        let health_checker = FileHealthChecker {
            file_path: tmp_dir.path().join("missing_file"),
        };

        let err = health_checker
            .check_health()
            .expect_err("checker should fail if the file is missing");

        assert!(err.to_string().contains("reading health file"))
    }

    #[test]
    fn fail_when_file_have_wrong_format() {
        struct TestCase {
            name: &'static str,
            file_content: &'static str,
        }
        impl TestCase {
            fn run(&self) {
                let tmp_dir = TempDir::new().unwrap();
                let mut file = File::create_new(tmp_dir.path().join("test")).unwrap();
                file.write_all(self.file_content.as_bytes()).unwrap();

                let health_checker = FileHealthChecker {
                    file_path: tmp_dir.path().join("test"),
                };
                let err = health_checker
                    .check_health()
                    .expect_err("checker should fail if the file has wrong format");

                assert!(
                    err.to_string().contains("parsing health file"),
                    "test case: {}",
                    self.name
                );
            }
        }

        let test_cases = vec![
            TestCase {
                name: "missing all",
                file_content: r#"
corrupted content    
"#,
            },
            TestCase {
                name: "missing status",
                file_content: r#"
healthy: true
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444001               
"#,
            },
            TestCase {
                name: "missing start_time",
                file_content: r#"
healthy: true
status: "some agent-specific message"
status_time_unix_nano: 1725444001               
"#,
            },
            TestCase {
                name: "missing status_time",
                file_content: r#"
healthy: true
status: "some agent-specific message"
start_time_unix_nano: 1725444001               
"#,
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }
}
