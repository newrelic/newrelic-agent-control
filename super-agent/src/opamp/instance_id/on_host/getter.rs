use crate::opamp::instance_id::getter::ULIDInstanceIDGetter;
use crate::opamp::instance_id::on_host::storer::{Storer, StorerError};
use identifier::{system::detector::SystemDetector, Detect};
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
    D: Detect<2>,
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
    D: Detect<2>,
{
    pub fn provide(&self) -> Identifiers {
        let mut identifiers = self.system_detector.detect();
        let hostname = std::mem::replace(&mut identifiers.attributes[0].1, Ok("".to_string()))
            .unwrap_or_else(|e| {
                error!("cannot get hostname identifier: {}", e.to_string());
                "".into()
            });
        let machine_id = std::mem::replace(&mut identifiers.attributes[1].1, Ok("".to_string()))
            .unwrap_or_else(|e| {
                error!("cannot get machine_id identifier: {}", e.to_string());
                "".into()
            });
        Identifiers {
            hostname,
            machine_id,
        }
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
    use identifier::system::detector::SystemDetectorError;
    use identifier::Detect;
    use identifier::{DetectError, Resource};
    use tracing_test::internal::logs_with_scope_contain;
    use tracing_test::traced_test;

    struct SystemDetectorMock {
        pub detect_mock: fn() -> Resource<2>,
    }

    impl Detect<2> for SystemDetectorMock {
        fn detect(&self) -> Resource<2> {
            (self.detect_mock)()
        }
    }

    #[traced_test]
    #[test]
    fn test_hostname_error_will_return_empty_hostname() {
        let identifiers_provider = IdentifiersProvider {
            system_detector: SystemDetectorMock {
                detect_mock: || -> Resource<2> {
                    Resource {
                        attributes: [
                            (
                                "hostname".to_string(),
                                Err(DetectError::from(SystemDetectorError::HostnameError(
                                    "errno".to_string(),
                                ))),
                            ),
                            ("machine-i".to_string(), Ok("some machine id".to_string())),
                        ],
                    }
                },
            },
        };
        let identifiers = identifiers_provider.provide();

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
        let identifiers_provider = IdentifiersProvider {
            system_detector: SystemDetectorMock {
                detect_mock: || -> Resource<2> {
                    Resource {
                        attributes: [
                            ("hostname".to_string(), Ok("some.example.org".to_string())),
                            (
                                "machine-i".to_string(),
                                Err(DetectError::SystemError(
                                    SystemDetectorError::HostnameError(String::from(
                                        "machine-id was not found...",
                                    )),
                                )),
                            ),
                        ],
                    }
                },
            },
        };
        let identifiers = identifiers_provider.provide();

        let expected_identifiers = Identifiers {
            hostname: String::from("some.example.org"),
            machine_id: String::from(""),
        };
        assert_eq!(expected_identifiers, identifiers);
        assert!(logs_with_scope_contain(
            "test_machine_id_error_will_return_empty_machine_id",
            "machine-id was not found..."
        ));
    }

    #[traced_test]
    #[test]
    fn test_providers_should_be_returned() {
        let identifiers_provider = IdentifiersProvider {
            system_detector: SystemDetectorMock {
                detect_mock: || -> Resource<2> {
                    Resource {
                        attributes: [
                            ("hostname".to_string(), Ok("some.example.org".to_string())),
                            ("machine-i".to_string(), Ok(String::from("some machine-id"))),
                        ],
                    }
                },
            },
        };
        let identifiers = identifiers_provider.provide();

        let expected_identifiers = Identifiers {
            hostname: String::from("some.example.org"),
            machine_id: String::from("some machine-id"),
        };
        assert_eq!(expected_identifiers, identifiers);
    }
}
