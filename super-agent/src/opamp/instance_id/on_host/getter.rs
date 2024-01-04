use crate::opamp::instance_id::getter::ULIDInstanceIDGetter;
use crate::opamp::instance_id::on_host::storer::{Storer, StorerError};
use resource_detection::cloud::aws::detector::AWSDetector;
use resource_detection::cloud::azure::detector::AzureDetector;
use resource_detection::cloud::http_client::HttpClientUreq;
use resource_detection::cloud::{AWS_INSTANCE_ID, AZURE_INSTANCE_ID};
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

pub struct IdentifiersProvider<
    D = SystemDetector,
    D2 = AWSDetector<HttpClientUreq>,
    D3 = AzureDetector<HttpClientUreq>,
> where
    D: Detect,
    D2: Detect,
    D3: Detect,
{
    system_detector: D,
    aws_cloud_detector: D2,
    azure_cloud_detector: D3,
}

impl Default for IdentifiersProvider {
    fn default() -> Self {
        Self {
            system_detector: SystemDetector::default(),
            aws_cloud_detector: AWSDetector::default(),
            azure_cloud_detector: AzureDetector::default(),
        }
    }
}

impl<D, D2, D3> IdentifiersProvider<D, D2, D3>
where
    D: Detect,
    D2: Detect,
    D3: Detect,
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

        Ok(Identifiers {
            hostname,
            machine_id,
            cloud_instance_id: self.cloud_instance_id(),
        })
    }

    // Try to get cloud instance_id from different cloud providers
    fn cloud_instance_id(&self) -> String {
        // TODO: should we propagate cloud error?
        let aws_cloud_instance_id: String = self
            .aws_cloud_detector
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
                error!("aws cloud detector error: {}", e);
                "".to_string()
            });

        if !aws_cloud_instance_id.is_empty() {
            return aws_cloud_instance_id;
        }

        // TODO: should we propagate cloud error?
        let azure_cloud_instance_id: String = self
            .azure_cloud_detector
            .detect()
            .map(|c_identifiers| {
                c_identifiers
                    .get(AZURE_INSTANCE_ID.into())
                    .map(|val| val.into())
                    .unwrap_or_else(|| {
                        error!("cannot get azure identifier");
                        "".to_string()
                    })
            })
            .unwrap_or_else(|e| {
                error!("aws cloud detector error: {}", e);
                "".to_string()
            });

        azure_cloud_instance_id
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
    use resource_detection::cloud::aws::detector::AWSDetectorError;
    use resource_detection::cloud::http_client::HttpClientError;
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

    impl MockCloudDetectorMock {
        fn should_detect(&mut self, resource: Resource) {
            self.expect_detect()
                .once()
                .return_once(move || Ok(resource));
        }

        fn should_not_detect(&mut self, error: DetectError) {
            self.expect_detect().once().return_once(move || Err(error));
        }
    }

    #[traced_test]
    #[test]
    fn test_hostname_error_will_return_empty_hostname() {
        let mut system_detector_mock = MockSystemDetectorMock::new();
        let mut aws_detector_mock = MockCloudDetectorMock::new();
        let azure_detector_mock = MockCloudDetectorMock::new();
        system_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource::new([(
                "machine_id".to_string().into(),
                Value::from("some machine id".to_string()),
            )]))
        });
        aws_detector_mock.should_detect(Resource::new([(
            "aws_instance_id".to_string().into(),
            Value::from("abc".to_string()),
        )]));

        let identifiers_provider = IdentifiersProvider {
            system_detector: system_detector_mock,
            aws_cloud_detector: aws_detector_mock,
            azure_cloud_detector: azure_detector_mock,
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
        let mut aws_cloud_detector_mock = MockCloudDetectorMock::new();
        let azure_cloud_detector_mock = MockCloudDetectorMock::new();
        aws_cloud_detector_mock
            .expect_detect()
            .once()
            .returning(|| {
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
            aws_cloud_detector: aws_cloud_detector_mock,
            azure_cloud_detector: azure_cloud_detector_mock,
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
        let mut aws_cloud_detector_mock = MockCloudDetectorMock::new();
        let azure_cloud_detector_mock = MockCloudDetectorMock::new();
        aws_cloud_detector_mock
            .expect_detect()
            .once()
            .returning(|| {
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
            aws_cloud_detector: aws_cloud_detector_mock,
            azure_cloud_detector: azure_cloud_detector_mock,
        };
        let identifiers = identifiers_provider.provide().unwrap();

        let expected_identifiers = Identifiers {
            hostname: String::from("some.example.org"),
            machine_id: String::from("some machine-id"),
            cloud_instance_id: String::from("abc"),
        };
        assert_eq!(expected_identifiers, identifiers);
    }

    #[traced_test]
    #[test]
    fn azure_provider_should_be_fetched_if_aws_not_present() {
        let mut system_detector_mock = MockSystemDetectorMock::new();
        let mut aws_cloud_detector_mock = MockCloudDetectorMock::new();
        let mut azure_cloud_detector_mock = MockCloudDetectorMock::new();
        azure_cloud_detector_mock.should_detect(Resource::new([(
            "azure_instance_id".to_string().into(),
            Value::from("an-azure-instance-id".to_string()),
        )]));

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
        aws_cloud_detector_mock.should_not_detect(DetectError::AWSError(
            AWSDetectorError::HttpError(HttpClientError::UreqError(String::from(
                "not an aws instance",
            ))),
        ));

        let identifiers_provider = IdentifiersProvider {
            system_detector: system_detector_mock,
            aws_cloud_detector: aws_cloud_detector_mock,
            azure_cloud_detector: azure_cloud_detector_mock,
        };
        let identifiers = identifiers_provider.provide().unwrap();

        let expected_identifiers = Identifiers {
            hostname: String::from("some.example.org"),
            machine_id: String::from("some machine-id"),
            cloud_instance_id: String::from("an-azure-instance-id"),
        };
        assert_eq!(expected_identifiers, identifiers);
    }
}
