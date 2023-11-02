use futures::{future, StreamExt};
use std::error::Error;
use tracing::{info, trace, warn};

use kube::{
    api::{Api, ResourceExt},
    core::{DynamicObject, GroupVersionKind},
    runtime::{
        reflector::{self, Store},
        watcher, WatchStreamExt,
    },
    Client,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();

    let client = Client::try_default().await?;

    // The example requires the Foo CRD to be installed
    // $ kubectl apply -f manifests/foo-crd.yaml

    // Usually, reader/writer can be created using `reflector::store` but it requires knowing the resource
    // kind, version and group at compile-time.
    // Check out the example at <https://github.com/kube-rs/kube/blob/5813ad043e00e7b34de5e22a3fd983419ece2493/examples/crd_reflector.rs#L36>

    // Define (at runtime) group, version and kind, so we can get the `api` and `api_resource`.
    let gvk = GroupVersionKind::gvk("clux.dev", "v1", "Foo");
    // Use API discovery to identify more information about the type (like its plural)
    let (api_resource, _caps) = kube::discovery::pinned_kind(&client, &gvk).await?;
    let api: Api<DynamicObject> = Api::default_namespaced_with(client, &api_resource);

    // Create reader and writer
    let writer = reflector::store::Writer::<DynamicObject>::new(api_resource.clone());
    let reader = writer.as_reader();

    // The watcher config can define filtering (labels, fields, ...), we list all Foo CRs in this example
    let watcher_config = watcher::Config::default().any_semantic();

    // polling the events is needed to keep the reader updated
    let writer_task = tokio::spawn(poll_events(writer, api, watcher_config));
    let reader_task = tokio::spawn(read_crs_periodically(reader));

    // This is an example of how to clean up the tokio tasks.
    // Details: <https://github.com/tokio-rs/tokio/discussions/5534>
    // We could use channels or signals if `abort()` is not enough.
    tokio::signal::ctrl_c().await?;
    info!("Cleaning up tasks...");
    reader_task.abort();
    writer_task.abort();
    info!("Bye!");
    Ok(())
}

// Defines a watcher to poll all CR events and reflects changes in the writer.
async fn poll_events(
    writer: reflector::store::Writer<DynamicObject>,
    api: Api<DynamicObject>,
    watcher_config: watcher::Config,
) {
    // TODO. check watcher's memory usage <https://docs.rs/kube/latest/kube/runtime/fn.reflector.html#memory-usage>
    // `metadata_watcher` could be another option. Would it be feasible?
    watcher(api, watcher_config)
        .default_backoff()
        .reflect(writer)
        .touched_objects()
        .for_each(|o| {
            if let Some(e) = o.err() {
                // Errors ar supposed to be recoverable: <https://docs.rs/kube/latest/kube/runtime/fn.watcher.html#recovery>
                warn!("Error polling events: {e}")
            }
            future::ready(())
        })
        .await
}

// Reads the state periodically
// We can create/edit/delete CRs to check how it works. For example:
// $ kubectl apply -f manifest/crd-bax.yaml
async fn read_crs_periodically(reader: Store<DynamicObject>) {
    reader.wait_until_ready().await.unwrap();
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        info!("CRs state:");
        reader.state().iter().for_each(|r| {
            info!(
                " - Foo with name: {} and labels {:?}",
                r.name_any(),
                r.labels(),
            );
            let yaml = serde_yaml::to_string(r.as_ref()).unwrap();
            trace!("yaml: {}", yaml);
        });
    }
}
