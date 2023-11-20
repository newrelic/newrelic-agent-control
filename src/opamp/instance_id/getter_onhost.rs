use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct Identifiers {
    pub hostname: String,
    pub machine_id: String,
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use crate::opamp::instance_id::getter::{DataStored, InstanceIDGetter, ULIDInstanceIDGetter};
    use crate::opamp::instance_id::storer::test::MockInstanceIDStorerMock;
    use crate::opamp::instance_id::storer::StorerError;

    #[test]
    fn test_not_found() {
        let mut mock = MockInstanceIDStorerMock::new();

        mock.expect_get().once().returning(|_| Ok(None));
        mock.expect_set().once().returning(|_, _| Ok(()));
        let getter = ULIDInstanceIDGetter::new(mock);
        let res = getter.get("agent_fqdn", &Identifiers::default());

        assert!(res.is_ok());
    }

    #[test]
    fn test_error_get() {
        let mut mock = MockInstanceIDStorerMock::new();

        mock.expect_get()
            .once()
            .returning(|_| Err(StorerError::Generic));
        let getter = ULIDInstanceIDGetter::new(mock);
        let res = getter.get("agent_fqdn", &Identifiers::default());

        assert!(res.is_err());
    }

    #[test]
    fn test_error_set() {
        let mut mock = MockInstanceIDStorerMock::new();

        mock.expect_get().once().returning(|_| Ok(None));
        mock.expect_set()
            .once()
            .returning(|_, _| Err(StorerError::Generic));

        let getter = ULIDInstanceIDGetter::new(mock);
        let res = getter.get("agent_fqdn", &Identifiers::default());

        assert!(res.is_err());
    }

    #[test]
    fn test_ulid_already_present() {
        let mut mock = MockInstanceIDStorerMock::new();
        let ulid = ulid::Ulid::new();

        mock.expect_get().once().returning(move |_| {
            Ok(Some(DataStored {
                ulid,
                identifiers: Default::default(),
            }))
        });
        let getter = ULIDInstanceIDGetter::new(mock);
        let res = getter.get("agent_fqdn", &Identifiers::default());

        assert!(res.is_ok());
        assert_eq!(ulid.to_string(), res.unwrap());
    }

    #[test]
    fn test_ulid_present_but_different_identifiers() {
        let agent_fqdn = "agent.example.com";
        let mut mock = MockInstanceIDStorerMock::new();
        let ulid = ulid::Ulid::new();

        mock.expect_get().once().returning(move |_| {
            Ok(Some(DataStored {
                ulid,
                identifiers: Identifiers {
                    machine_id: "different".to_string(),
                    hostname: "different".to_string(),
                },
            }))
        });
        mock.expect_set().once().returning(|_, _| Ok(()));
        let getter = ULIDInstanceIDGetter::new(mock);
        let res = getter.get(agent_fqdn, &Identifiers::default());

        assert!(res.is_ok());
        assert_ne!(ulid.to_string(), res.unwrap());
    }
}
