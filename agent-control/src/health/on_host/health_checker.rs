use super::exec::ExecHealthChecker;
use super::file::FileHealthChecker;
use super::http::HttpHealthChecker;
use crate::agent_type::runtime_config::health_config::OnHostHealthCheck;
use crate::event::channel::EventConsumer;
use crate::health::health_checker::{HealthChecker, HealthCheckerError, Healthy};
use crate::health::with_start_time::{HealthWithStartTime, StartTime};
use crate::http::client::HttpClient;
use std::path::PathBuf;

pub enum OnHostHealthChecker {
    Exec(ExecHealthChecker),
    Http(HttpHealthChecker),
    File(FileHealthChecker),
}
pub struct OnHostHealthCheckers {
    health_checkers: Vec<OnHostHealthChecker>,
    start_time: StartTime,
}

impl OnHostHealthCheckers {
    pub(crate) fn try_new(
        exec_health_consumer: EventConsumer<(String, HealthWithStartTime)>,
        http_client: HttpClient,
        health_check_type: Option<OnHostHealthCheck>,
        start_time: StartTime,
    ) -> Result<Self, HealthCheckerError> {
        let mut health_checkers = vec![OnHostHealthChecker::Exec(ExecHealthChecker::new(
            exec_health_consumer,
        ))];
        match health_check_type {
            Some(OnHostHealthCheck::HttpHealth(http_config)) => {
                health_checkers.push(OnHostHealthChecker::Http(HttpHealthChecker::new(
                    http_client,
                    http_config,
                    start_time,
                )?));
            }
            Some(OnHostHealthCheck::FileHealth(file_config)) => {
                health_checkers.push(OnHostHealthChecker::File(FileHealthChecker::new(
                    PathBuf::from(file_config.path),
                )));
            }
            _ => {}
        }
        Ok(OnHostHealthCheckers {
            health_checkers,
            start_time,
        })
    }
}

impl HealthChecker for OnHostHealthCheckers {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        let mut status = "".to_string();
        for checker in &self.health_checkers {
            let health = match checker {
                OnHostHealthChecker::Exec(exec_checker) => exec_checker.check_health()?,
                OnHostHealthChecker::Http(http_checker) => http_checker.check_health()?,
                OnHostHealthChecker::File(file_checker) => file_checker.check_health()?,
            };

            // We are overriding the status with any status from the health checks that is not empty.
            // At the moment this status is not used (only in tests) but this part should be revisited
            // doing a concatenation on the format decided whenever this status starts to get parsed.
            if !health.status().is_empty() {
                status = health.status().to_string();
            }

            if !health.is_healthy() {
                return Ok(health);
            }
        }

        Ok(HealthWithStartTime::from_healthy(
            Healthy::new().with_status(status),
            self.start_time,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::channel::pub_sub;
    use crate::health::health_checker::Unhealthy;
    use crate::health::with_start_time::StartTime;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_check_health_all_healthy() {
        let start_time = StartTime::now();
        let (exec_health_publisher, exec_health_consumer) = pub_sub();
        let _ = exec_health_publisher.publish((
            "exec1".to_string(),
            HealthWithStartTime::new(Healthy::new().into(), start_time),
        ));
        let tmp_dir = TempDir::new().unwrap();
        let mut file = File::create_new(tmp_dir.path().join("test")).unwrap();
        file.write_all(
            r#"
healthy: true
status: "some agent-specific message"
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444001
"#
            .as_bytes(),
        )
        .unwrap();

        let file_health_checker = FileHealthChecker::new(tmp_dir.path().join("test"));

        let health_checkers = vec![
            OnHostHealthChecker::Exec(ExecHealthChecker::new(exec_health_consumer)),
            OnHostHealthChecker::File(file_health_checker),
        ];

        let on_host_health_checkers = OnHostHealthCheckers {
            health_checkers,
            start_time,
        };

        let result = on_host_health_checkers.check_health();
        assert!(result.is_ok());
        assert!(result.unwrap().is_healthy());
    }

    #[test]
    fn test_check_health_exec_unhealthy() {
        let start_time = StartTime::now();
        let (exec_health_publisher, exec_health_consumer) = pub_sub();
        let unhealthy = Unhealthy::new("Error1".to_string());
        let _ = exec_health_publisher.publish((
            "exec1".to_string(),
            HealthWithStartTime::new(unhealthy.into(), start_time),
        ));
        let tmp_dir = TempDir::new().unwrap();
        let mut file = File::create_new(tmp_dir.path().join("test")).unwrap();
        file.write_all(
            r#"
healthy: true
status: "some agent-specific message"
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444001
"#
            .as_bytes(),
        )
        .unwrap();

        let file_health_checker = FileHealthChecker::new(tmp_dir.path().join("test"));

        let health_checkers = vec![
            OnHostHealthChecker::Exec(ExecHealthChecker::new(exec_health_consumer)),
            OnHostHealthChecker::File(file_health_checker),
        ];

        let on_host_health_checkers = OnHostHealthCheckers {
            health_checkers,
            start_time,
        };

        let result = on_host_health_checkers.check_health();
        assert!(result.is_ok());
        assert!(!result.unwrap().is_healthy());
    }

    #[test]
    fn test_check_health_file_unhealthy() {
        let start_time = StartTime::now();
        let (exec_health_publisher, exec_health_consumer) = pub_sub();
        let _ = exec_health_publisher.publish((
            "exec1".to_string(),
            HealthWithStartTime::new(Healthy::new().into(), start_time),
        ));
        let tmp_dir = TempDir::new().unwrap();
        let mut file = File::create_new(tmp_dir.path().join("test")).unwrap();
        file.write_all(
            r#"
healthy: false
status: "some agent-specific message"
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444001
"#
            .as_bytes(),
        )
        .unwrap();

        let file_health_checker = FileHealthChecker::new(tmp_dir.path().join("test"));

        let health_checkers = vec![
            OnHostHealthChecker::Exec(ExecHealthChecker::new(exec_health_consumer)),
            OnHostHealthChecker::File(file_health_checker),
        ];

        let on_host_health_checkers = OnHostHealthCheckers {
            health_checkers,
            start_time,
        };

        let result = on_host_health_checkers.check_health();
        assert!(result.is_ok());
        assert!(!result.unwrap().is_healthy());
    }

    #[test]
    fn test_check_health_with_multiple_non_empty_statuses() {
        let start_time = StartTime::now();
        let (exec_health_publisher, exec_health_consumer) = pub_sub();

        // First health check with a non-empty status
        let _ = exec_health_publisher.publish((
            "exec1".to_string(),
            HealthWithStartTime::new(
                Healthy::new()
                    .with_status("first non-empty status message".to_string())
                    .into(),
                start_time,
            ),
        ));

        let tmp_dir = TempDir::new().unwrap();
        let mut file = File::create_new(tmp_dir.path().join("test")).unwrap();

        // Second health check with a different non-empty status
        file.write_all(
            r#"
healthy: true
status: "second non-empty status message"
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444001
"#
            .as_bytes(),
        )
        .unwrap();

        let file_health_checker = FileHealthChecker::new(tmp_dir.path().join("test"));

        let health_checkers = vec![
            OnHostHealthChecker::Exec(ExecHealthChecker::new(exec_health_consumer)),
            OnHostHealthChecker::File(file_health_checker),
        ];

        let on_host_health_checkers = OnHostHealthCheckers {
            health_checkers,
            start_time,
        };

        let result = on_host_health_checkers.check_health();
        assert!(result.is_ok());
        let health_with_start_time = result.unwrap();
        assert!(health_with_start_time.is_healthy());
        assert_eq!(
            health_with_start_time.status(),
            "second non-empty status message"
        );
    }

    #[test]
    fn test_check_health_returns_first_unhealthy() {
        let start_time = StartTime::now();
        let (exec_health_publisher, exec_health_consumer) = pub_sub();

        // First health check with a non-empty status
        let _ = exec_health_publisher.publish((
            "exec1".to_string(),
            HealthWithStartTime::new(Unhealthy::new("exec error".to_string()).into(), start_time),
        ));

        let tmp_dir = TempDir::new().unwrap();
        let mut file = File::create_new(tmp_dir.path().join("test")).unwrap();

        // Second health check with a different non-empty status
        file.write_all(
            r#"
healthy: false
status: "file status"
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444001
"#
            .as_bytes(),
        )
        .unwrap();

        let file_health_checker = FileHealthChecker::new(tmp_dir.path().join("test"));

        let health_checkers = vec![
            OnHostHealthChecker::Exec(ExecHealthChecker::new(exec_health_consumer)),
            OnHostHealthChecker::File(file_health_checker),
        ];

        let on_host_health_checkers = OnHostHealthCheckers {
            health_checkers,
            start_time,
        };

        let result = on_host_health_checkers.check_health();
        assert!(result.is_ok());
        let health_with_start_time = result.unwrap();
        assert!(!health_with_start_time.is_healthy());
        assert_eq!(
            health_with_start_time.last_error(),
            Some("executable exec1 failed: exec error".to_string())
        );
    }
}
