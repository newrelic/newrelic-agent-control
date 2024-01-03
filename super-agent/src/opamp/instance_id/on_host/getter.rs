use crate::opamp::instance_id::getter::ULIDInstanceIDGetter;
use crate::opamp::instance_id::on_host::storer::{Storer, StorerError};
use resource_detection::cloud::aws::detector::AWSDetector;
use resource_detection::cloud::AWS_INSTANCE_ID;
use resource_detection::http_client::HttpClientUreq;
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
    pub cloud_instance_id: String,
}

pub struct IdentifiersProvider<D = SystemDetector, D2 = AWSDetector<HttpClientUreq>>
where
    D: Detect,
    D2: Detect,
{
    system_detector: D,
    cloud_detector: D2,
}

impl Default for IdentifiersProvider {
    fn default() -> Self {
        Self {
            system_detector: SystemDetector::default(),
            cloud_detector: AWSDetector::default(),
        }
    }
}

impl<D, D2> IdentifiersProvider<D, D2>
where
    D: Detect,
    D2: Detect,
{
    pub fn provide(&self) -> Result<Identifiers, DetectError> {
        let system_identifiers = self.system_detector.detect()?;
        let hostname: String = system_identifiers
            .get(HOSTNAME_KEY.into())
            .map(|val| val.into())
            .unwrap_or_else(|| {
                error!("cannot get hostname identifier");
                "".to_string()
            });
        let machine_id: String = system_identifiers
            .get(MACHINE_ID_KEY.into())
            .map(|val| val.into())
            .unwrap_or_else(|| {
                error!("cannot get machine_id identifier");
                "".to_string()
            });

        // TODO: should we propagate cloud error?
        let cloud_instance_id: String = self
            .cloud_detector
            .detect()
            .map(|c_identifiers| {
                c_identifiers
                    .get(AWS_INSTANCE_ID.into())
                    .map(|val| val.into())
                    .unwrap_or_else(|| {
                        error!("cannot get aws identifier");
                        "".to_string()
                    })
            })
            .unwrap_or_else(|e| {
                error!("cloud detector error: {}", e);
                "".to_string()
            });

        Ok(Identifiers {
            hostname,
            machine_id,
            cloud_instance_id,
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

    mock! {
        pub CloudDetectorMock {}
        impl Detect for CloudDetectorMock {
            fn detect(&self) -> Result<Resource, DetectError>;
        }
    }

    #[traced_test]
    #[test]
    fn test_hostname_error_will_return_empty_hostname() {
        let mut system_detector_mock = MockSystemDetectorMock::new();
        let mut cloud_detector_mock = MockCloudDetectorMock::new();
        system_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource::new([(
                "machine_id".to_string().into(),
                Value::from("some machine id".to_string()),
            )]))
        });
        cloud_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource::new([(
                "aws_instance_id".to_string().into(),
                Value::from("abc".to_string()),
            )]))
        });

        let identifiers_provider = IdentifiersProvider {
            system_detector: system_detector_mock,
            cloud_detector: cloud_detector_mock,
        };
        let identifiers = identifiers_provider.provide().unwrap();

        let expected_identifiers = Identifiers {
            hostname: String::from(""),
            machine_id: String::from("some machine id"),
            cloud_instance_id: String::from("abc"),
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
        let mut cloud_detector_mock = MockCloudDetectorMock::new();
        cloud_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource::new([(
                "aws_instance_id".to_string().into(),
                Value::from("abc".to_string()),
            )]))
        });
        system_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource::new([(
                Key::from("hostname".to_string()),
                Value::from("some.example.org".to_string()),
            )]))
        });
        let identifiers_provider = IdentifiersProvider {
            system_detector: system_detector_mock,
            cloud_detector: cloud_detector_mock,
        };
        let identifiers = identifiers_provider.provide().unwrap();

        let expected_identifiers = Identifiers {
            hostname: String::from("some.example.org"),
            machine_id: String::from(""),
            cloud_instance_id: String::from("abc"),
        };
        assert_eq!(expected_identifiers, identifiers);
        assert!(logs_with_scope_contain(
            "test_machine_id_error_will_return_empty_machine_id",
            "cannot get machine_id identifier"
        ));
    }

    #[traced_test]
    #[test]
    fn test_all_providers_should_be_returned() {
        let mut system_detector_mock = MockSystemDetectorMock::new();
        let mut cloud_detector_mock = MockCloudDetectorMock::new();
        cloud_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource::new([(
                "aws_instance_id".to_string().into(),
                Value::from("abc".to_string()),
            )]))
        });
        system_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource::new([
                (
                    Key::from("hostname".to_string()),
                    Value::from("some.example.org".to_string()),
                ),
                (
                    "machine_id".to_string().into(),
                    Value::from("some machine-id".to_string()),
                ),
            ]))
        });
        let identifiers_provider = IdentifiersProvider {
            system_detector: system_detector_mock,
            cloud_detector: cloud_detector_mock,
        };
        let identifiers = identifiers_provider.provide().unwrap();

        let expected_identifiers = Identifiers {
            hostname: String::from("some.example.org"),
            machine_id: String::from("some machine-id"),
            cloud_instance_id: String::from("abc"),
        };
        assert_eq!(expected_identifiers, identifiers);
    }
}
