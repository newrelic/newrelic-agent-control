use crate::opamp::instance_id::getter::ULIDInstanceIDGetter;
#[cfg_attr(test, mockall_double::double)]
use crate::opamp::instance_id::on_host::identifier_machine_id_unix::IdentifierProviderMachineId;
use crate::opamp::instance_id::on_host::storer::{Storer, StorerError};
#[cfg_attr(test, mockall_double::double)]
use crate::utils::hostname::HostnameGetter;
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

#[derive(Default)]
pub struct IdentifiersProvider {
    hostname_getter: HostnameGetter,
    machine_id_provider: IdentifierProviderMachineId,
}

impl IdentifiersProvider {
    pub fn provide(&self) -> Identifiers {
        Identifiers {
            hostname: self.hostname_identifier(),
            machine_id: self.machine_id_identifier(),
        }
    }

    fn hostname_identifier(&self) -> String {
        self.hostname_getter
            .get()
            .unwrap_or_else(|e| {
                error!("cannot get hostname identifier: {}", e.to_string());
                "".into()
            })
            .into_string()
            .unwrap()
    }

    fn machine_id_identifier(&self) -> String {
        self.machine_id_provider.provide().unwrap_or_else(|e| {
            error!("cannot get machine_id identifier: {}", e.to_string());
            "".to_string()
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
    use crate::opamp::instance_id::on_host::getter::{
        IdentifierRetrievalError, IdentifiersProvider,
    };
    use crate::opamp::instance_id::on_host::identifier_machine_id_unix::MockIdentifierProviderMachineId;
    use crate::opamp::instance_id::Identifiers;
    use crate::utils::hostname::MockHostnameGetter;
    use nix::errno::Errno;
    use tracing_test::internal::logs_with_scope_contain;
    use tracing_test::traced_test;

    impl IdentifiersProvider {
        fn new(
            hostname_provider: MockHostnameGetter,
            machine_id_provider: MockIdentifierProviderMachineId,
        ) -> Self {
            Self {
                hostname_getter: hostname_provider,
                machine_id_provider,
            }
        }
    }

    #[traced_test]
    #[test]
    fn test_hostname_error_will_return_empty_hostname() {
        let mut hostname_getter = MockHostnameGetter::default();
        let mut machine_id_provider = MockIdentifierProviderMachineId::default();

        hostname_getter.should_not_get(Errno::EBUSY);
        machine_id_provider.should_provide(String::from("some machine id"));

        let identifiers_provider = IdentifiersProvider::new(hostname_getter, machine_id_provider);
        let identifiers = identifiers_provider.provide();

        let expected_identifiers = Identifiers {
            hostname: String::from(""),
            machine_id: String::from("some machine id"),
        };
        assert_eq!(expected_identifiers, identifiers);
        assert!(logs_with_scope_contain(
            "test_hostname_error_will_return_empty_hostname",
            "cannot get hostname identifier: EBUSY: Device or resource busy"
        ));
    }

    #[traced_test]
    #[test]
    fn test_machine_id_error_will_return_empty_machine_id() {
        let mut hostname_getter = MockHostnameGetter::default();
        let mut machine_id_provider = MockIdentifierProviderMachineId::default();

        hostname_getter.should_get(String::from("some.example.org"));
        machine_id_provider.should_not_provide(IdentifierRetrievalError::HostnameError(
            String::from("machine-id was not found..."),
        ));

        let identifiers_provider = IdentifiersProvider::new(hostname_getter, machine_id_provider);
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
        let mut hostname_provider = MockHostnameGetter::default();
        let mut machine_id_provider = MockIdentifierProviderMachineId::default();

        hostname_provider.should_get(String::from("some.example.org"));
        machine_id_provider.should_provide(String::from("some machine-id"));

        let identifiers_provider = IdentifiersProvider::new(hostname_provider, machine_id_provider);
        let identifiers = identifiers_provider.provide();

        let expected_identifiers = Identifiers {
            hostname: String::from("some.example.org"),
            machine_id: String::from("some machine-id"),
        };
        assert_eq!(expected_identifiers, identifiers);
    }
}
