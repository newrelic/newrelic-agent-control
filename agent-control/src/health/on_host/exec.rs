use crate::health::health_checker::{HealthChecker, HealthCheckerError, Healthy, Unhealthy};
use crate::health::with_start_time::HealthWithStartTime;
use crate::sub_agent::on_host::health::repository::ExecHealthRepository;
use std::sync::Arc;
use std::time::SystemTime;

pub struct ExecHealthChecker<E: ExecHealthRepository + Send + Sync + 'static> {
    pub(super) exec_health_repository: Arc<E>,
}

impl<E: ExecHealthRepository + Send + Sync + 'static> ExecHealthChecker<E> {
    pub fn new(exec_health_repository: Arc<E>) -> Self {
        Self {
            exec_health_repository,
        }
    }
}

impl<E: ExecHealthRepository + Send + Sync + 'static> HealthChecker for ExecHealthChecker<E> {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        let execs_health = self
            .exec_health_repository
            .all()
            .map_err(|err| HealthCheckerError::Generic(format!("getting exec health: {err}")))?;
        let mut start_time = SystemTime::now();
        let errors: Vec<String> = execs_health
            .iter()
            .filter_map(|(name, health)| {
                let error = health
                    .as_health()
                    .last_error()
                    .map(|e| format!("{name}: {e}"));
                if health.start_time() < start_time {
                    start_time = health.start_time();
                }
                error
            })
            .collect();

        let errors_str = errors.join(", ");
        let healthiness = errors.is_empty();

        if !healthiness {
            return Ok(HealthWithStartTime::new(
                Unhealthy::new(errors_str).into(),
                start_time,
            ));
        }

        Ok(HealthWithStartTime::new(Healthy::new().into(), start_time))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::health_checker::{Healthy, Unhealthy};
    use crate::health::with_start_time::HealthWithStartTime;
    use crate::sub_agent::on_host::health::repository::ExecHealthRepository;
    use std::collections::HashMap;
    use std::time::{Duration, SystemTime};
    struct MockExecHealthRepository {
        health_data: HashMap<String, HealthWithStartTime>,
    }

    impl ExecHealthRepository for MockExecHealthRepository {
        type Error = HealthCheckerError;

        fn set(
            &self,
            _executable: String,
            _health: HealthWithStartTime,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn get(&self, _executable: String) -> Result<Option<HealthWithStartTime>, Self::Error> {
            Ok(None)
        }

        fn all(&self) -> Result<HashMap<String, HealthWithStartTime>, HealthCheckerError> {
            Ok(self.health_data.clone())
        }
    }

    #[test]
    fn test_check_health_all_healthy() {
        let mut health_data = HashMap::new();
        health_data.insert(
            "exec1".to_string(),
            HealthWithStartTime::new(Healthy::default().into(), SystemTime::now()),
        );
        health_data.insert(
            "exec2".to_string(),
            HealthWithStartTime::new(Healthy::default().into(), SystemTime::now()),
        );

        let repository = MockExecHealthRepository { health_data };
        let checker = ExecHealthChecker::new(Arc::new(repository));

        let result = checker.check_health().unwrap();
        assert!(result.is_healthy());
    }

    #[test]
    fn test_check_health_one_unhealthy() {
        let mut health_data = HashMap::new();
        health_data.insert(
            "exec1".to_string(),
            HealthWithStartTime::new(
                Unhealthy::default()
                    .with_last_error("Error1".to_string())
                    .into(),
                SystemTime::now(),
            ),
        );
        health_data.insert(
            "exec2".to_string(),
            HealthWithStartTime::new(Healthy::default().into(), SystemTime::now()),
        );

        let repository = MockExecHealthRepository { health_data };
        let checker = ExecHealthChecker::new(Arc::new(repository));

        let result = checker.check_health().unwrap();
        assert!(!result.is_healthy());
        assert_eq!(result.as_health().last_error(), Some("exec1: Error1"));
    }

    #[test]
    fn test_check_health_two_unhealthy() {
        let mut health_data = HashMap::new();
        health_data.insert(
            "exec1".to_string(),
            HealthWithStartTime::new(
                Unhealthy::default()
                    .with_last_error("Error1".to_string())
                    .into(),
                SystemTime::now(),
            ),
        );
        health_data.insert(
            "exec2".to_string(),
            HealthWithStartTime::new(
                Unhealthy::default()
                    .with_last_error("Error2".to_string())
                    .into(),
                SystemTime::now(),
            ),
        );
        health_data.insert(
            "exec3".to_string(),
            HealthWithStartTime::new(Healthy::default().into(), SystemTime::now()),
        );

        let repository = MockExecHealthRepository { health_data };
        let checker = ExecHealthChecker::new(Arc::new(repository));

        let result = checker.check_health().unwrap();
        assert!(!result.is_healthy());

        // The initial input is a hashmap & we can't ensure order so we must assert on contains
        assert!(
            result
                .as_health()
                .last_error()
                .unwrap()
                .contains("exec1: Error1")
        );
        assert!(result.as_health().last_error().unwrap().contains(", "));
        assert!(
            result
                .as_health()
                .last_error()
                .unwrap()
                .contains("exec2: Error2")
        );
    }

    #[test]
    fn test_check_health_start_time() {
        let start_time1 = SystemTime::now() - Duration::new(60, 0);
        let start_time2 = SystemTime::now();

        let mut health_data = HashMap::new();
        health_data.insert(
            "exec1".to_string(),
            HealthWithStartTime::new(Healthy::default().into(), start_time1),
        );
        health_data.insert(
            "exec2".to_string(),
            HealthWithStartTime::new(Healthy::default().into(), start_time2),
        );

        let repository = MockExecHealthRepository { health_data };
        let checker = ExecHealthChecker::new(Arc::new(repository));

        let result = checker.check_health().unwrap();
        assert_eq!(result.start_time(), start_time1);
    }
}
