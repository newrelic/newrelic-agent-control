use crate::super_agent::instance_id_storer::InstanceIDStorer;
use log::debug;
use serde::{Deserialize, Serialize};
use std::cmp::Eq;
use ulid::Ulid;

#[feature(async_fn_in_trait)]
pub trait InstanceIDGetter {
    type Identifier: Metadata;
    async fn get(&self, id: &str, identifiers: &Self::Identifier) -> String;
}

pub trait Metadata: Default + Eq + PartialEq + Serialize + Clone {}

async fn cache_or_new<T, K>(persister: &K, id: &str, identifiers: &T) -> String
where
    T: Metadata,
    K: InstanceIDStorer<Identifier = T>,
{
    let new_data: DataStored<T>;

    debug!("retrieving ulid");
    let cache_data_result = persister.get(id).await;
    match cache_data_result {
        Ok(data) => {
            if data.identifiers == *identifiers {
                return data.ulid;
            }
            new_data = new_ulid(identifiers)
        }
        Err(err) => {
            debug!("failed to retrieve ulid data {}", err);
            new_data = new_ulid(identifiers)
        }
    }

    debug!("persisting ulid {}", new_data.ulid.to_string());

    let _ = persister
        .set(id, &new_data)
        .await
        .map_err(|err| debug!("failed to persist ulid {}", new_data.ulid.to_string()));

    new_data.ulid
}

fn new_ulid<T>(identifiers: &T) -> DataStored<T>
where
    T: Metadata,
{
    let ulid = Ulid::new().to_string();
    DataStored {
        ulid: ulid.to_owned(),
        identifiers: identifiers.clone(),
    }
}

#[derive(Deserialize, Serialize, Eq, PartialEq)]
pub struct DataStored<T>
where
    T: Metadata,
{
    ulid: String,
    identifiers: T,
}

///
/// Kubernetes Implementation
///

#[derive(Default, Deserialize, Serialize, Eq, PartialEq, Clone)]
pub struct K8sIdentifiers {
    pub hostname: String,
    pub machine_id: String,
}

impl Metadata for K8sIdentifiers {}

#[derive(Default)]
pub struct K8sULIDInstanceIDGetter<T>
where
    T: InstanceIDStorer,
{
    pub persister: T,
}

impl<T> InstanceIDGetter for K8sULIDInstanceIDGetter<T>
where
    T: InstanceIDStorer<Identifier = K8sIdentifiers>,
{
    type Identifier = K8sIdentifiers;

    async fn get(&self, id: &str, identifiers: &Self::Identifier) -> String {
        cache_or_new::<K8sIdentifiers, T>(&self.persister, id, identifiers).await
    }
}

///
/// OnHost implementation
///
#[derive(Default, Deserialize, Serialize, Eq, PartialEq, Clone)]
pub struct OnHostIdentifiers {
    pub hostname: String,
    pub machine_id: String,
}

impl Metadata for OnHostIdentifiers {}

#[derive(Default)]
pub struct OnHostULIDInstanceIDGetter<T>
where
    T: InstanceIDStorer,
{
    pub persister: T,
}

impl<T> InstanceIDGetter for OnHostULIDInstanceIDGetter<T>
where
    T: InstanceIDStorer<Identifier = OnHostIdentifiers>,
{
    type Identifier = OnHostIdentifiers;

    async fn get(&self, id: &str, identifiers: &Self::Identifier) -> String {
        cache_or_new::<OnHostIdentifiers, T>(&self.persister, id, identifiers).await
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use crate::k8s::executor::K8sExecutor;
    use crate::super_agent::instance_id_getter::K8sULIDInstanceIDGetter;
    use crate::super_agent::instance_id_storer::K8sStorer;
    use mockall::mock;
    use mockall::*;

    #[tokio::test]
    async fn test_test() {
        let c = K8sExecutor::try_default().await.unwrap();

        let mut a = K8sULIDInstanceIDGetter::<K8sStorer> {
            persister: K8sStorer {
                configmap_name: "test-first".to_string(),
                k8s_executor: c,
            },
        };

        a.get(
            "first",
            &K8sIdentifiers {
                hostname: "value1".to_string(),
                machine_id: "value2".to_string(),
            },
        )
        .await;

        a.get(
            "second",
            &K8sIdentifiers {
                hostname: "value1".to_string(),
                machine_id: "value2".to_string(),
            },
        )
        .await;

        a.get(
            "second",
            &K8sIdentifiers {
                hostname: "value1".to_string(),
                machine_id: "value3".to_string(),
            },
        )
        .await;
    }

    mock! {
        pub InstanceIDGetterMock {}

        impl InstanceIDGetter for InstanceIDGetterMock {
            type Identifier = OnHostIdentifiers;

            async fn get(&self, id:&str, identifiers: &OnHostIdentifiers) -> String;
        }
    }

    //   impl MockInstanceIDGetterMock {
    // pub fn should_get(
    //     &mut self,
    //     name: String,
    //     instance_id: String,
    //     identifiers: OnHostIdentifiers,
    // ) {
    //     self.expect_get()
    //         .once()
    //         .with(predicate::eq(name.clone()))
    //         .returning(move |_| instance_id.clone());
    // }
    // }
}
