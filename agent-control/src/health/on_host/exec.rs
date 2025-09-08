use crate::event::channel::EventConsumer;
use crate::health::health_checker::{HealthChecker, HealthCheckerError, Healthy, Unhealthy};
use crate::health::with_start_time::HealthWithStartTime;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::SystemTime;

pub struct ExecHealthChecker {
    exec_health_consumer: EventConsumer<(String, HealthWithStartTime)>,
    execs_health: RefCell<HashMap<String, HealthWithStartTime>>,
}

impl ExecHealthChecker {
    pub fn new(exec_health_consumer: EventConsumer<(String, HealthWithStartTime)>) -> Self {
        Self {
            exec_health_consumer,
            execs_health: RefCell::new(HashMap::new()),
        }
    }
}

impl HealthChecker for ExecHealthChecker {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        let mut execs_health = self.execs_health.borrow_mut();

        while let Ok(health_message) = self.exec_health_consumer.as_ref().try_recv() {
            execs_health.insert(health_message.0, health_message.1);
        }

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
    use crate::event::channel::pub_sub;
    use crate::health::health_checker::{Healthy, Unhealthy};
    use crate::health::with_start_time::HealthWithStartTime;
    use std::time::{Duration, SystemTime};

    #[test]
    fn test_check_health_all_healthy() {
        let (health_publisher, health_consumer) = pub_sub();
        let supervisor_start_time = SystemTime::now();

        let init_health = Healthy::new();
        let _ = health_publisher.publish((
            "exec1".to_string(),
            HealthWithStartTime::new(init_health.clone().into(), supervisor_start_time),
        ));
        let _ = health_publisher.publish((
            "exec2".to_string(),
            HealthWithStartTime::new(init_health.into(), supervisor_start_time),
        ));

        let checker = ExecHealthChecker::new(health_consumer);

        let result = checker.check_health().unwrap();
        assert!(result.as_health().is_healthy());
    }

    #[test]
    fn test_check_health_one_unhealthy() {
        let (health_publisher, health_consumer) = pub_sub();
        let supervisor_start_time = SystemTime::now();

        let unhealthy = Unhealthy::new("Error1".to_string());
        let _ = health_publisher.publish((
            "exec1".to_string(),
            HealthWithStartTime::new(unhealthy.into(), supervisor_start_time),
        ));
        let init_health = Healthy::new();
        let _ = health_publisher.publish((
            "exec2".to_string(),
            HealthWithStartTime::new(init_health.into(), supervisor_start_time),
        ));

        let checker = ExecHealthChecker::new(health_consumer);

        let result = checker.check_health().unwrap();
        assert!(!result.as_health().is_healthy());
        assert_eq!(result.as_health().last_error().unwrap(), "exec1: Error1");
    }

    #[test]
    fn test_check_health_two_unhealthy() {
        let (health_publisher, health_consumer) = pub_sub();
        let supervisor_start_time = SystemTime::now();

        let unhealthy1 = Unhealthy::new("Error1".to_string());
        let _ = health_publisher.publish((
            "exec1".to_string(),
            HealthWithStartTime::new(unhealthy1.into(), supervisor_start_time),
        ));
        let unhealthy2 = Unhealthy::new("Error2".to_string());
        let _ = health_publisher.publish((
            "exec2".to_string(),
            HealthWithStartTime::new(unhealthy2.into(), supervisor_start_time),
        ));
        let init_health = Healthy::new();
        let _ = health_publisher.publish((
            "exec3".to_string(),
            HealthWithStartTime::new(init_health.into(), supervisor_start_time),
        ));

        let checker = ExecHealthChecker::new(health_consumer);

        let result = checker.check_health().unwrap();
        assert!(!result.as_health().is_healthy());

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
        let (health_publisher, health_consumer) = pub_sub();
        let start_time1 = SystemTime::now() - Duration::new(60, 0);
        let start_time2 = SystemTime::now();

        let init_health = Healthy::new();
        let _ = health_publisher.publish((
            "exec1".to_string(),
            HealthWithStartTime::new(init_health.clone().into(), start_time1),
        ));
        let _ = health_publisher.publish((
            "exec2".to_string(),
            HealthWithStartTime::new(init_health.into(), start_time2),
        ));

        let checker = ExecHealthChecker::new(health_consumer);

        let result = checker.check_health().unwrap();
        assert_eq!(result.start_time(), start_time1);
    }
}
