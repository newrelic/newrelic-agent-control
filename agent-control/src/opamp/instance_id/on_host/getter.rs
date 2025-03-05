use crate::http::client::HttpClient;
use crate::opamp::instance_id::on_host::storer::StorerError;
use resource_detection::cloud::aws::detector::{
    AWSDetector, AWS_IPV4_METADATA_ENDPOINT, AWS_IPV4_METADATA_TOKEN_ENDPOINT,
};
use resource_detection::cloud::azure::detector::{AzureDetector, AZURE_IPV4_METADATA_ENDPOINT};
use resource_detection::cloud::cloud_id::detector::CloudIdDetector;
use resource_detection::cloud::gcp::detector::{GCPDetector, GCP_IPV4_METADATA_ENDPOINT};
use resource_detection::cloud::http_client::HttpClientError;
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

#[derive(Error, Debug)]
pub enum IdentifiersProviderError {
    #[error("generating host identification: adding a `host_id` in the agent-control config is required for this case`")]
    MissingHostIDError,
    #[error("detecting resources: `{0}`")]
    DetectError(#[from] DetectError),
    #[error("Building cloud detector: `{0}`")]
    BuildError(#[from] HttpClientError),
}

pub struct IdentifiersProvider<
    D = SystemDetector,
    D2 = CloudIdDetector<
        AWSDetector<HttpClient>,
        AzureDetector<HttpClient>,
        GCPDetector<HttpClient>,
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

impl IdentifiersProvider {
    pub fn try_new(
        azure_http_client: HttpClient,
        aws_http_client: HttpClient,
        gcp_http_client: HttpClient,
    ) -> Result<Self, IdentifiersProviderError> {
        Ok(Self {
            system_detector: SystemDetector::default(),
            cloud_id_detector: CloudIdDetector::try_new(
                azure_http_client,
                aws_http_client,
                gcp_http_client,
                AWS_IPV4_METADATA_ENDPOINT.to_string(),
                AWS_IPV4_METADATA_TOKEN_ENDPOINT.to_string(),
                AZURE_IPV4_METADATA_ENDPOINT.to_string(),
                GCP_IPV4_METADATA_ENDPOINT.to_string(),
            )?,
            host_id: String::default(),
            fleet_id: String::default(),
        })
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

    pub fn provide(&self) -> Result<Identifiers, IdentifiersProviderError> {
        let system_identifiers = self.system_detector.detect()?;

        let hostname: String = system_identifiers
            .get(HOSTNAME_KEY.into())
            .map(|val| val.into())
            .unwrap_or_default();
        let machine_id: String = system_identifiers
            .get(MACHINE_ID_KEY.into())
            .map(|val| val.into())
            .unwrap_or_default();
        let cloud_instance_id = self.cloud_instance_id();

        // host_id is an aggregated identifier required by newrelic fleet management
        // to identify the host entity.
        // It is populated by the following precedence order:
        //   config defined host_id -> cloud instance id -> machine id
        let host_id = if self.host_id.is_empty() {
            if cloud_instance_id.is_empty() {
                machine_id.clone()
            } else {
                cloud_instance_id.clone()
            }
        } else {
            self.host_id.clone()
        };

        if host_id.is_empty() {
            return Err(IdentifiersProviderError::MissingHostIDError);
        }

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

#[cfg(test)]
pub mod tests {
    use crate::opamp::instance_id::Identifiers;
    use crate::opamp::instance_id::{
        on_host::getter::IdentifiersProvider, IdentifiersProviderError,
    };
    use assert_matches::assert_matches;
    use mockall::mock;
    use resource_detection::{DetectError, Detector, Key, Resource, Value};

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

    const CLOUD_ID: &str = "cloud_id";
    const HOSTNAME: &str = "hostname";
    const MACHINE_ID: &str = "machine_id";

    fn cloud_id() -> Resource {
        Resource::new([(
            Key::from("cloud_instance_id".to_string()),
            Value::from(CLOUD_ID.to_string()),
        )])
    }
    fn system_id() -> Resource {
        Resource::new([
            (
                Key::from("hostname".to_string()),
                Value::from(HOSTNAME.to_string()),
            ),
            (
                Key::from("machine_id".to_string()),
                Value::from(MACHINE_ID.to_string()),
            ),
        ])
    }

    #[test]
    fn test_provide_cases() {
        let host_id = "host_id".to_string();

        struct TestCase {
            name: &'static str,
            system_detector_mock: MockSystemDetectorMock,
            cloud_id_detector_mock: MockCloudDetectorMock,
            expected_identifiers: Identifiers,
            host_id: String,
        }
        impl TestCase {
            fn run(self) {
                let identifiers_provider = IdentifiersProvider {
                    system_detector: self.system_detector_mock,
                    cloud_id_detector: self.cloud_id_detector_mock,
                    host_id: self.host_id,
                    fleet_id: String::new(),
                };
                let identifiers = identifiers_provider.provide().expect(self.name);

                assert_eq!(
                    self.expected_identifiers, identifiers,
                    "test case: {}",
                    self.name
                );
            }
        }
        let test_cases = vec![
            TestCase {
                name: "configured host_id takes precedence over cloud id",
                host_id: host_id.clone(),
                system_detector_mock: {
                    let mut system_detector_mock = MockSystemDetectorMock::new();
                    system_detector_mock
                        .expect_detect()
                        .once()
                        .returning(|| Ok(system_id()));
                    system_detector_mock
                },
                cloud_id_detector_mock: {
                    let mut cloud_id_detector_mock = MockCloudDetectorMock::new();
                    cloud_id_detector_mock.should_detect(cloud_id());
                    cloud_id_detector_mock
                },
                expected_identifiers: Identifiers {
                    host_id: host_id.clone(),
                    hostname: HOSTNAME.to_string(),
                    machine_id: MACHINE_ID.to_string(),
                    cloud_instance_id: CLOUD_ID.to_string(),
                    ..Default::default()
                },
            },
            TestCase {
                name: "cloud id takes precedence over machine id",
                host_id: "".to_string(),
                system_detector_mock: {
                    let mut system_detector_mock = MockSystemDetectorMock::new();
                    system_detector_mock
                        .expect_detect()
                        .once()
                        .returning(|| Ok(system_id()));
                    system_detector_mock
                },
                cloud_id_detector_mock: {
                    let mut cloud_id_detector_mock = MockCloudDetectorMock::new();
                    cloud_id_detector_mock.should_detect(cloud_id());
                    cloud_id_detector_mock
                },
                expected_identifiers: Identifiers {
                    host_id: CLOUD_ID.to_string(),
                    hostname: HOSTNAME.to_string(),
                    machine_id: MACHINE_ID.to_string(),
                    cloud_instance_id: CLOUD_ID.to_string(),
                    ..Default::default()
                },
            },
            TestCase {
                name: "machine id as host_id",
                host_id: "".to_string(),
                system_detector_mock: {
                    let mut system_detector_mock = MockSystemDetectorMock::new();
                    system_detector_mock.expect_detect().once().returning(|| {
                        Ok(Resource::new([(
                            Key::from("machine_id".to_string()),
                            Value::from(MACHINE_ID.to_string()),
                        )]))
                    });
                    system_detector_mock
                },
                cloud_id_detector_mock: {
                    let mut cloud_id_detector_mock = MockCloudDetectorMock::new();
                    cloud_id_detector_mock.should_detect(Resource::new([]));
                    cloud_id_detector_mock
                },
                expected_identifiers: Identifiers {
                    host_id: MACHINE_ID.to_string(),
                    machine_id: MACHINE_ID.to_string(),
                    ..Default::default()
                },
            },
            TestCase {
                name: "configured host_id is the only required resource",
                host_id: host_id.clone(),
                system_detector_mock: {
                    let mut system_detector_mock = MockSystemDetectorMock::new();
                    system_detector_mock
                        .expect_detect()
                        .once()
                        .returning(|| Ok(Resource::new([])));
                    system_detector_mock
                },
                cloud_id_detector_mock: {
                    let mut cloud_id_detector_mock = MockCloudDetectorMock::new();
                    cloud_id_detector_mock.should_detect(Resource::new([]));
                    cloud_id_detector_mock
                },
                expected_identifiers: Identifiers {
                    host_id: host_id.clone(),
                    ..Default::default()
                },
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    #[test]
    fn test_empty_host_id_will_error() {
        let mut system_detector_mock = MockSystemDetectorMock::new();
        let mut cloud_id_detector_mock = MockCloudDetectorMock::new();
        cloud_id_detector_mock.should_detect(Resource::new([]));
        system_detector_mock
            .expect_detect()
            .once()
            .returning(|| Ok(Resource::new([])));

        let identifiers_provider = IdentifiersProvider {
            system_detector: system_detector_mock,
            cloud_id_detector: cloud_id_detector_mock,
            host_id: String::new(),
            fleet_id: String::new(),
        };

        let err = identifiers_provider
            .provide()
            .expect_err("empty host_id should fail");

        assert_matches!(err, IdentifiersProviderError::MissingHostIDError);
    }
}
