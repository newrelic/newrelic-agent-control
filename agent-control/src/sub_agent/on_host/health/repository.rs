use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use crate::health::with_start_time::HealthWithStartTime;

pub trait ExecHealthRepository {
    type Error;

    fn set(
        &self,
        executable: &str,
        health: HealthWithStartTime,
    ) -> Result<(), Self::Error>;

    fn get(&self, executable: String)
           -> Result<Option<HealthWithStartTime>, Self::Error>;

    fn all(&self)
           -> Result<HashMap<String, HealthWithStartTime>, Self::Error>;
}

#[derive(Error, Debug)]
pub enum InMemoryExecHealthError {
    #[error("failed to acquire lock")]
    LockError,
}

#[derive(Debug, Default)]
pub struct InMemoryExecHealthRepository {
    health_map: Arc<Mutex<HashMap<String, HealthWithStartTime>>>,
}

impl ExecHealthRepository for InMemoryExecHealthRepository {
    type Error = InMemoryExecHealthError;

    fn set(&self, executable: String, health: HealthWithStartTime) -> Result<(), Self::Error> {
        let mut map = self.health_map.lock().map_err(|_| InMemoryExecHealthError::LockError)?;
        map.insert(executable, health);
        Ok(())
    }

    fn get(&self, executable: String) -> Result<Option<HealthWithStartTime>, Self::Error> {
        let map = self.health_map.lock().map_err(|_| InMemoryExecHealthError::LockError)?;
        Ok(map.get(&executable).cloned())
    }

    fn all(&self) -> Result<HashMap<String, HealthWithStartTime>, Self::Error> {
        let map = self.health_map.lock().map_err(|_| InMemoryExecHealthError::LockError)?;
        Ok(map.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::with_start_time::HealthWithStartTime;
    use std::collections::HashMap;
    use std::time::SystemTime;
    use crate::health::health_checker::Healthy;

    #[test]
    fn test_set_and_get() {
        let repository = InMemoryExecHealthRepository::default();
        let executable = "test_executable".to_string();
        let health = HealthWithStartTime::new(
            Healthy::default().into(),
            SystemTime::now(),
        );
        assert!(repository.set(executable.clone(), health.clone()).is_ok());

        let retrieved_health = repository.get(executable.clone()).unwrap();
        assert_eq!(retrieved_health, Some(health));
    }

    #[test]
    fn test_get_non_existent() {
        let repository = InMemoryExecHealthRepository::default();
        let executable = "non_existent_executable".to_string();

        // Test getting health for a non-existent executable
        let retrieved_health = repository.get(executable).unwrap();
        assert_eq!(retrieved_health, None);
    }

    #[test]
    fn test_all() {
        let repository = InMemoryExecHealthRepository::default();
        let executable1 = "executable1".to_string();
        let executable2 = "executable2".to_string();
        let health1 = HealthWithStartTime::new(
            Healthy::default().into(),
            SystemTime::now(),
        );
        let health2 = HealthWithStartTime::new(
            Healthy::default().into(),
            SystemTime::now(),
        );

        // Set health for multiple executables
        assert!(repository.set(executable1.clone(), health1.clone()).is_ok());
        assert!(repository.set(executable2.clone(), health2.clone()).is_ok());

        // Test retrieving all health data
        let all_health = repository.all().unwrap();
        let mut expected_health = HashMap::new();
        expected_health.insert(executable1, health1);
        expected_health.insert(executable2, health2);

        assert_eq!(all_health, expected_health);
    }
}