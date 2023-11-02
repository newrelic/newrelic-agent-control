use std::error::Error;

use kube::{
    core::{DynamicObject, GroupVersionKind},
    Api, Client, ResourceExt,
};
use tracing::info;

// This example assumes the Foo CRD already exists. It can be created using:
// $ kubectl apply -f manifests/foo-crd.yaml
// It handles CRS whose group, version and kind are known at runtime.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();
    let client = Client::try_default().await?;

    let gvk = GroupVersionKind::gvk("clux.dev", "v1", "Foo");

    // Use API discovery to identify more information about the type (like its plural)
    let (api_resource, _caps) = kube::discovery::pinned_kind(&client, &gvk).await?;

    let api: Api<DynamicObject> = Api::default_namespaced_with(client, &api_resource);

    let foo_id = ulid::Ulid::new().to_string().to_lowercase();

    let cr_definition = format!(
        r#"
apiVersion: clux.dev/v1
kind: Foo
metadata:
  name: {foo_id}
  labels:
    l1: v1
spec:
  name: {foo_id}
  info: this is {foo_id}
"#
    );

    let cr_from_yaml: DynamicObject = serde_yaml::from_str(&cr_definition)?;

    // Created (first time)
    update_or_create_cr(&api, &foo_id, cr_from_yaml.clone(), "k2", "v2").await?;

    show_foo_crs(&api).await?;

    // Updated (as it already exists)
    update_or_create_cr(&api, &foo_id, cr_from_yaml, "k3", "v3").await?;

    show_foo_crs(&api).await?;

    info!("Delete {foo_id} Foo");
    api.delete(&foo_id, &Default::default())
        .await?
        .map_left(|cr| {
            info!("Deleting {foo_id}: {:?}", cr.metadata.deletion_timestamp);
        })
        .map_right(|s| {
            info!("Deleted {foo_id}: {:?}", s);
        });

    show_foo_crs(&api).await?;

    Ok(())
}

// This could be generic for any Api<K> where K implements Resource.
// It doesn't use reflectors/informers, so the API is triggered every time.
async fn show_foo_crs(api: &Api<DynamicObject>) -> Result<(), Box<dyn Error>> {
    info!("List all Foo CRs");
    api.list(&Default::default())
        .await?
        .into_iter()
        .for_each(|cr| {
            info!(
                "-  Found CR with name '{}' and labels: {:?}",
                cr.name_any(),
                cr.labels()
            );
        });
    Ok(())
}

async fn update_or_create_cr(
    api: &Api<DynamicObject>,
    name: &str,
    cr_definition: DynamicObject,
    key_label: &str,
    value_label: &str,
) -> Result<(), Box<dyn Error>> {
    info!("Update or create {name} Foo, and include the label {key_label}:{value_label}");
    api.entry(name)
        .await?
        // apply the change if it already exists
        .and_modify(|cr| {
            cr.labels_mut()
                .insert(key_label.to_string(), value_label.to_string());
        })
        .or_insert(|| cr_definition)
        // apply the change even if we have just created it the CR
        .and_modify(|cr| {
            cr.labels_mut()
                .insert(key_label.to_string(), value_label.to_string());
        })
        // persists the changes
        .commit(&Default::default())
        .await?;
    Ok(())
}
