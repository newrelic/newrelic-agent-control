use crate::opamp::instance_id::getter::InstanceIDWithIdentifiersGetter;
use crate::opamp::instance_id::on_host::storer::{Storer, StorerError};
use resource_detection::cloud::aws::detector::AWSDetector;
use resource_detection::cloud::azure::detector::AzureDetector;
use resource_detection::cloud::cloud_id::detector::CloudIdDetector;
use resource_detection::cloud::gcp::detector::GCPDetector;
use resource_detection::cloud::http_client::HttpClientUreq;
use resource_detection::cloud::CLOUD_INSTANCE_ID;
use resource_detection::system::{HOSTNAME_KEY, MACHINE_ID_KEY};
use resource_detection::DetectError;
use resource_detection::{system::detector::SystemDetector, Detector};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use thiserror::Error;
use tracing::error;

#[derive(Default, Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct Identifiers {
    pub hostname: String,
    pub machine_id: String,
    pub cloud_instance_id: String,
    pub host_id: String,
    pub fleet_id: String,
}

impl Display for Identifiers {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "hostname = '{}', machine_id = '{}', cloud_instance_id = '{}', host_id = '{}', fleet_id = '{}'",
            self.hostname, self.machine_id, self.cloud_instance_id, self.host_id,self.fleet_id,
        )
    }
}

pub struct IdentifiersProvider<
    D = SystemDetector,
    D2 = CloudIdDetector<
        AWSDetector<HttpClientUreq>,
        AzureDetector<HttpClientUreq>,
        GCPDetector<HttpClientUreq>,
    >,
> where
    D: Detector,
    D2: Detector,
{
    system_detector: D,
    cloud_id_detector: D2,
    host_id: String,
    fleet_id: String,
}

impl Default for IdentifiersProvider {
    fn default() -> Self {
        Self {
            system_detector: SystemDetector::default(),
            cloud_id_detector: CloudIdDetector::default(),
            host_id: String::default(),
            fleet_id: String::default(),
        }
    }
}

impl<D, D2> IdentifiersProvider<D, D2>
where
    D: Detector,
    D2: Detector,
{
    pub fn with_host_id(self, host_id: String) -> Self {
        Self { host_id, ..self }
    }

    pub fn with_fleet_id(self, fleet_id: String) -> Self {
        Self { fleet_id, ..self }
    }

    pub fn new(system_detector: D, cloud_id_detector: D2) -> Self {
        Self {
            system_detector,
            cloud_id_detector,
            host_id: String::default(),
            fleet_id: String::default(),
        }
    }

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
        let cloud_instance_id = self.cloud_instance_id();

        // It's possible that the Host ID was set up early (via config).
        // If this is the case, we don't want to overwrite it.
        let host_id = if self.host_id.is_empty() {
            if cloud_instance_id.is_empty() {
                machine_id.clone()
            } else {
                cloud_instance_id.clone()
            }
        } else {
            self.host_id.clone()
        };

        Ok(Identifiers {
            // https://opentelemetry.io/docs/specs/semconv/resource/host/#collecting-hostid-from-non-containerized-systems
            host_id,
            hostname,
            machine_id,
            cloud_instance_id,
            fleet_id: self.fleet_id.clone(),
        })
    }

    // Try to get cloud instance_id from different cloud providers
    fn cloud_instance_id(&self) -> String {
        // TODO: should we propagate cloud error?
        self.cloud_id_detector
            .detect()
            .map(|c_identifiers| {
                c_identifiers
                    .get(CLOUD_INSTANCE_ID.into())
                    .map(|val| val.into())
                    .unwrap_or_else(|| {
                        error!("cannot get cloud id identifier");
                        "".to_string()
                    })
            })
            .unwrap_or_else(|e| {
                error!("aws cloud detector error: {}", e);
                "".to_string()
            })
    }
}

#[derive(Error, Debug)]
pub enum GetterError {
    #[error("failed to persist Data: `{0}`")]
    Persisting(#[from] StorerError),
}

impl Default for InstanceIDWithIdentifiersGetter<Storer> {
    fn default() -> Self {
        Self::new(Storer::default(), Identifiers::default())
    }
}

#[cfg(test)]
pub mod test {
    use crate::opamp::instance_id::on_host::getter::IdentifiersProvider;
    use crate::opamp::instance_id::Identifiers;
    use mockall::mock;
    use resource_detection::{DetectError, Detector, Key, Resource, Value};
    use tracing_test::internal::logs_with_scope_contain;
    use tracing_test::traced_test;

    mock! {
        pub SystemDetectorMock {}
        impl Detector for SystemDetectorMock {
            fn detect(&self) -> Result<Resource, DetectError>;
        }
    }

    mock! {
        pub CloudDetectorMock {}
        impl Detector for CloudDetectorMock {
            fn detect(&self) -> Result<Resource, DetectError>;
        }
    }

    impl MockSystemDetectorMock {
        pub fn should_detect(&mut self, resource: Resource) {
            self.expect_detect()
                .once()
                .return_once(move || Ok(resource));
        }

        pub fn should_fail_detection(&mut self, err: DetectError) {
            self.expect_detect().once().return_once(move || Err(err));
        }
    }

    impl MockCloudDetectorMock {
        pub fn should_detect(&mut self, resource: Resource) {
            self.expect_detect()
                .once()
                .return_once(move || Ok(resource));
        }

        pub fn should_fail_detection(&mut self, err: DetectError) {
            self.expect_detect().once().return_once(move || Err(err));
        }
    }

    #[traced_test]
    #[test]
    fn test_hostname_error_will_return_empty_hostname() {
        let mut system_detector_mock = MockSystemDetectorMock::new();
        let mut cloud_id_detector_mock = MockCloudDetectorMock::new();
        system_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource::new([(
                "machine_id".to_string().into(),
                Value::from("some machine id".to_string()),
            )]))
        });
        cloud_id_detector_mock.should_detect(Resource::new([(
            "cloud_instance_id".to_string().into(),
            Value::from("abc".to_string()),
        )]));

        let identifiers_provider = IdentifiersProvider {
            system_detector: system_detector_mock,
            cloud_id_detector: cloud_id_detector_mock,
            host_id: String::new(),
            fleet_id: String::new(),
        };
        let identifiers = identifiers_provider.provide().unwrap();

        let expected_identifiers = Identifiers {
            hostname: String::from(""),
            machine_id: String::from("some machine id"),
            cloud_instance_id: String::from("abc"),
            host_id: String::from("abc"),
            fleet_id: String::new(),
        };
        assert_eq!(expected_identifiers, identifiers);
        assert!(logs_with_scope_contain(
            "test_hostname_error_will_return_empty_hostname",
            "cannot get hostname identifier",
        ));
    }

    #[traced_test]
    #[test]
    fn test_machine_id_error_will_return_empty_machine_id() {
        let mut system_detector_mock = MockSystemDetectorMock::new();
        let mut cloud_id_detector_mock = MockCloudDetectorMock::new();
        cloud_id_detector_mock.should_detect(Resource::new([(
            "cloud_instance_id".to_string().into(),
            Value::from("abc".to_string()),
        )]));
        system_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource::new([(
                Key::from("hostname".to_string()),
                Value::from("some.example.org".to_string()),
            )]))
        });

        let identifiers_provider = IdentifiersProvider {
            system_detector: system_detector_mock,
            cloud_id_detector: cloud_id_detector_mock,
            host_id: String::new(),
            fleet_id: String::new(),
        };
        let identifiers = identifiers_provider.provide().unwrap();

        let expected_identifiers = Identifiers {
            hostname: String::from("some.example.org"),
            machine_id: String::from(""),
            cloud_instance_id: String::from("abc"),
            host_id: String::from("abc"),
            fleet_id: String::new(),
        };
        assert_eq!(expected_identifiers, identifiers);
        assert!(logs_with_scope_contain(
            "test_machine_id_error_will_return_empty_machine_id",
            "cannot get machine_id identifier",
        ));
    }

    #[traced_test]
    #[test]
    fn test_host_id_fallback() {
        let mut system_detector_mock = MockSystemDetectorMock::new();
        let mut cloud_id_detector_mock = MockCloudDetectorMock::new();
        // empty cloud_id
        cloud_id_detector_mock.should_detect(Resource::new([(
            "cloud_instance_id".to_string().into(),
            Value::from("".to_string()),
        )]));
        system_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource::new([(
                "machine_id".to_string().into(),
                Value::from("some machine id".to_string()),
            )]))
        });

        let identifiers_provider = IdentifiersProvider {
            system_detector: system_detector_mock,
            cloud_id_detector: cloud_id_detector_mock,
            host_id: String::new(),
            fleet_id: String::new(),
        };
        let identifiers = identifiers_provider.provide().unwrap();

        let expected_identifiers = Identifiers {
            hostname: String::from(""),
            machine_id: String::from("some machine id"),
            cloud_instance_id: String::from(""),
            host_id: String::from("some machine id"),
            fleet_id: String::new(),
        };
        assert_eq!(expected_identifiers, identifiers);
    }

    #[traced_test]
    #[test]
    fn test_all_providers_should_be_returned() {
        let mut system_detector_mock = MockSystemDetectorMock::new();
        let mut cloud_id_detector_mock = MockCloudDetectorMock::new();
        cloud_id_detector_mock.should_detect(Resource::new([(
            "cloud_instance_id".to_string().into(),
            Value::from("abc".to_string()),
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
        let identifiers_provider = IdentifiersProvider {
            system_detector: system_detector_mock,
            cloud_id_detector: cloud_id_detector_mock,
            host_id: String::new(),
            fleet_id: String::new(),
        };
        let identifiers = identifiers_provider.provide().unwrap();

        let expected_identifiers = Identifiers {
            hostname: String::from("some.example.org"),
            machine_id: String::from("some machine-id"),
            cloud_instance_id: String::from("abc"),
            host_id: String::from("abc"),
            fleet_id: String::new(),
        };
        assert_eq!(expected_identifiers, identifiers);
    }

    #[test]
    fn test_predefined_host_id_overrides() {
        let mut system_detector_mock = MockSystemDetectorMock::new();
        let mut cloud_id_detector_mock = MockCloudDetectorMock::new();
        cloud_id_detector_mock.should_detect(Resource::new([(
            "cloud_instance_id".to_string().into(),
            Value::from("abc".to_string()),
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
        let identifiers_provider = IdentifiersProvider {
            system_detector: system_detector_mock,
            cloud_id_detector: cloud_id_detector_mock,
            host_id: String::new(),
            fleet_id: String::new(),
        };
        // Add a host_id
        let identifiers_provider = identifiers_provider.with_host_id("some-host-id".to_string());

        let identifiers = identifiers_provider.provide().unwrap();
        assert_eq!(identifiers.host_id, "some-host-id");
    }
}
