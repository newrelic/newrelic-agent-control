use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{
    api::{DynamicObject, ObjectMeta, Patch, PatchParams, PostParams, TypeMeta},
    core::GroupVersion,
    runtime::reflector::Lookup,
    Api, Client, CustomResource, CustomResourceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, str::FromStr, time::Duration};
use tokio::sync::OnceCell;

// foo CRD is installed in the cluster by the k8s envrioment setup helper
#[derive(Default, CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(group = "newrelic.com", version = "v1", kind = "Foo", namespaced)]
pub struct FooSpec {
    pub data: String,
}

pub fn foo_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "newrelic.com/v1".to_string(),
        kind: "Foo".to_string(),
    }
}

pub async fn get_dynamic_api_foo(client: kube::Client, test_ns: String) -> Api<DynamicObject> {
    let gvk = &GroupVersion::from_str(foo_type_meta().api_version.as_str())
        .unwrap()
        .with_kind(foo_type_meta().kind.as_str());
    let (ar, _) = kube::discovery::pinned_kind(&client.to_owned(), gvk)
        .await
        .unwrap();
    let api: Api<DynamicObject> = Api::namespaced_with(client.to_owned(), test_ns.as_str(), &ar);
    api
}

/// Create the Foo CRD for testing purposes.The CRD is not cleaned on test termination (for simplicity) so all tests
/// can assume this CRD exists.
pub async fn create_foo_crd(client: Client) {
    static ONCE: OnceCell<()> = OnceCell::const_new();
    ONCE.get_or_try_init(|| async { create_crd(client, Foo::crd()).await })
        .await
        .expect("Error creating the Foo CRD");

    // Wait for the CRD to be fully deployed: https://github.com/kubernetes/kubectl/issues/1117
    tokio::time::sleep(Duration::from_secs(1)).await;
}

pub async fn create_crd(client: Client, crd: CustomResourceDefinition) -> Result<(), kube::Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client);
    let ssapply = PatchParams::apply("crd_apply_example").force();
    crds.patch(&crd.name().unwrap(), &ssapply, &Patch::Apply(crd.clone()))
        .await?;
    Ok(())
}

pub async fn delete_crd(client: Client, crd: CustomResourceDefinition) -> Result<(), kube::Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client);
    crds.delete(&crd.name().unwrap(), &Default::default())
        .await?;
    Ok(())
}

/// Creates a Foo CR for testing purposes.
/// ### Panics
/// It panics if there is an error creating the CR.
pub async fn create_foo_cr(
    client: Client,
    namespace: &str,
    name: &str,
    labels: Option<BTreeMap<String, String>>,
    annotations: Option<BTreeMap<String, String>>,
) -> Foo {
    let api: Api<Foo> = Api::namespaced(client, namespace);
    let mut foo_cr = Foo::new(
        name,
        FooSpec {
            data: String::from("test"),
        },
    );

    foo_cr.metadata.labels = labels;
    foo_cr.metadata.annotations = annotations;

    foo_cr = api.create(&PostParams::default(), &foo_cr).await.unwrap();

    // Sleeping to let watchers have the time to be updated
    tokio::time::sleep(Duration::from_secs(1)).await;

    foo_cr
}

/// Build a dynamic_object object from the provided values
pub fn build_dynamic_object(
    type_meta: TypeMeta,
    name: String,
    content: serde_json::Value,
) -> DynamicObject {
    DynamicObject {
        types: Some(type_meta),
        metadata: ObjectMeta {
            name: Some(name),
            ..Default::default()
        },
        data: content,
    }
}
