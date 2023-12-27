use crate::opamp::instance_id::getter::ULIDInstanceIDGetter;
use crate::opamp::instance_id::on_host::storer::{Storer, StorerError};
use resource_detection::system::{HOSTNAME_KEY, MACHINE_ID_KEY};
use resource_detection::DetectError;
use resource_detection::{system::detector::SystemDetector, Detect};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::error;

#[derive(Error, Debug)]
#[cfg_attr(test, derive(Clone))]
pub enum IdentifierRetrievalError {
    #[error("error getting hostname `{0}`")]
    HostnameError(String),
    #[error("error getting machine-id: `{0}`")]
    MachineIDError(String),
}

#[derive(Default, Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct Identifiers {
    pub hostname: String,
    pub machine_id: String,
}

pub struct IdentifiersProvider<D = SystemDetector>
where
    D: Detect,
{
    system_detector: D,
}

impl Default for IdentifiersProvider {
    fn default() -> Self {
        Self {
            system_detector: SystemDetector::default(),
        }
    }
}

impl<D> IdentifiersProvider<D>
where
    D: Detect,
{
    pub fn provide(&self) -> Result<Identifiers, DetectError> {
        let identifiers = self.system_detector.detect()?;
        let hostname: String = identifiers
            .get(HOSTNAME_KEY.into())
            .map(|val| val.into())
            .unwrap_or_else(|| {
                error!("cannot get hostname identifier");
                "".to_string()
            });
        let machine_id: String = identifiers
            .get(MACHINE_ID_KEY.into())
            .map(|val| val.into())
            .unwrap_or_else(|| {
                error!("cannot get machine_id identifier");
                "".to_string()
            });
        Ok(Identifiers {
            hostname,
            machine_id,
        })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum GetterError {
    #[error("failed to persist Data: `{0}`")]
    Persisting(#[from] StorerError),
}

impl Default for ULIDInstanceIDGetter<Storer> {
    fn default() -> Self {
        Self::new(Storer::default(), Identifiers::default())
    }
}

#[cfg(test)]
mod test {

    use crate::opamp::instance_id::on_host::getter::IdentifiersProvider;
    use crate::opamp::instance_id::Identifiers;
    use mockall::mock;
    use resource_detection::{Detect, DetectError, Key, Resource, Value};
    use tracing_test::internal::logs_with_scope_contain;
    use tracing_test::traced_test;

    mock! {
        pub SystemDetectorMock {}
        impl Detect for SystemDetectorMock {
            fn detect(&self) -> Result<Resource, DetectError>;
        }
    }

    #[traced_test]
    #[test]
    fn test_hostname_error_will_return_empty_hostname() {
        let mut system_detector_mock = MockSystemDetectorMock::new();
        system_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource {
                attributes: [(
                    "machine_id".to_string().into(),
                    Value::from("some machine id".to_string()),
                )]
                .into(),
            })
        });
        let identifiers_provider = IdentifiersProvider {
            system_detector: system_detector_mock,
        };
        let identifiers = identifiers_provider.provide().unwrap();

        let expected_identifiers = Identifiers {
            hostname: String::from(""),
            machine_id: String::from("some machine id"),
        };
        assert_eq!(expected_identifiers, identifiers);
        assert!(logs_with_scope_contain(
            "test_hostname_error_will_return_empty_hostname",
            "cannot get hostname identifier"
        ));
    }

    #[traced_test]
    #[test]
    fn test_machine_id_error_will_return_empty_machine_id() {
        let mut system_detector_mock = MockSystemDetectorMock::new();
        system_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource {
                attributes: [(
                    Key::from("hostname".to_string()),
                    Value::from("some.example.org".to_string()),
                )]
                .into(),
            })
        });
        let identifiers_provider = IdentifiersProvider {
            system_detector: system_detector_mock,
        };
        let identifiers = identifiers_provider.provide().unwrap();

        let expected_identifiers = Identifiers {
            hostname: String::from("some.example.org"),
            machine_id: String::from(""),
        };
        assert_eq!(expected_identifiers, identifiers);
        assert!(logs_with_scope_contain(
            "test_machine_id_error_will_return_empty_machine_id",
            "cannot get machine_id identifier"
        ));
    }

    #[traced_test]
    #[test]
    fn test_providers_should_be_returned() {
        let mut system_detector_mock = MockSystemDetectorMock::new();
        system_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource {
                attributes: [
                    (
                        Key::from("hostname".to_string()),
                        Value::from("some.example.org".to_string()),
                    ),
                    (
                        "machine_id".to_string().into(),
                        Value::from("some machine-id".to_string()),
                    ),
                ]
                .into(),
            })
        });
        let identifiers_provider = IdentifiersProvider {
            system_detector: system_detector_mock,
        };
        let identifiers = identifiers_provider.provide().unwrap();

        let expected_identifiers = Identifiers {
            hostname: String::from("some.example.org"),
            machine_id: String::from("some machine-id"),
        };
        assert_eq!(expected_identifiers, identifiers);
    }
}
