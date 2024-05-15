use futures::StreamExt;
use std::fmt::Debug;
use std::future;

use kube::{
    core::DynamicObject,
    discovery::ApiResource,
    runtime::{
        reflector::{self, store::Writer},
        watcher, WatchStreamExt,
    },
    Api, Client,
};

use serde::de::DeserializeOwned;
use tokio::task::{AbortHandle, JoinHandle};
use tracing::warn;

use super::{super::error::K8sError, resources::ResourceWithReflector};

/// Reflector builder holds the arguments to build a reflector.
/// Its implementation allows creating a reflector for supported types.
///
/// ## Example:
/// ```ignore
/// // We cannot run the example because of the dependencies
/// let builder = reflectors::ReflectorBuilder::new(client);
/// let dynamic_object_reflector = builder.try_build_with_api_resource(api_resource).unwrap();
/// let deployment_reflector = builder.try_build::<Deployment>().unwrap();
/// ```
pub struct ReflectorBuilder {
    client: Client,
}

impl ReflectorBuilder {
    /// Returns a reflector builder, consuming both the provided client and the namespace.
    pub fn new(client: Client) -> Self {
        ReflectorBuilder { client }
    }

    /// Builds the DynamicObject reflector using the builder.
    ///
    /// # Arguments
    /// * `api_resource` - The [ApiResource] corresponding to the required [DynamicObject].
    ///
    /// # Returns
    /// Returns the newly built reflector or an error.

    pub async fn try_build_with_api_resource(
        &self,
        api_resource: &ApiResource,
    ) -> Result<Reflector<DynamicObject>, K8sError> {
        // The api consumes the client, so it needs to be owned to allow sharing the builder.
        let api: Api<DynamicObject> =
            Api::default_namespaced_with(self.client.to_owned(), api_resource);

        // Initialize the writer for the dynamic type.
        let writer: Writer<DynamicObject> = reflector::store::Writer::new(api_resource.to_owned());

        Reflector::try_new(api, writer, self.watcher_config()).await
    }

    /// Builds a reflector using the builder.
    ///
    /// # Type Parameters
    /// * `K` - Kubernetes resource type implementing the required trait.
    ///
    /// # Returns
    /// Returns the newly built reflector or an error.
    pub async fn try_build<K>(&self) -> Result<Reflector<K>, K8sError>
    where
        K: ResourceWithReflector,
    {
        // Create an API instance for the resource type.
        let api: Api<K> = Api::default_namespaced(self.client.clone());

        // Initialize the writer for the resource type.
        let writer: Writer<K> = reflector::store::Writer::default();

        Reflector::try_new(api, writer, self.watcher_config()).await
    }

    /// Returns the watcher_config to use in reflectors
    pub fn watcher_config(&self) -> watcher::Config {
        Default::default()
    }
}

/// A generic Kubernetes reflector for resources implementing the [kube::core::Resource].
/// It works by keeping an internal reader-writer pair:
/// - The reader keeps a read-only cache of Kubernetes objects.
/// - The writer continuously updates the cache based on the API stream.
///
/// The writer's async task is aborted when the reflector is dropped.
#[derive(Debug)]
pub struct Reflector<K>
where
    K: kube::core::Resource + Clone + DeserializeOwned + Debug + Send + Sync + 'static,
    K::DynamicType: Eq + std::hash::Hash + Clone + Debug,
{
    /// The read-only store that maintains a cache of Kubernetes objects of type `K`.
    reader: reflector::Store<K>,
    /// Handle for the writer task, which updates the cache. Used to abort the task on drop.
    writer_close_handle: AbortHandle,
}

impl<K> Reflector<K>
where
    K: kube::core::Resource + Clone + DeserializeOwned + Debug + Send + Sync + 'static,
    K::DynamicType: Eq + std::hash::Hash + Clone + Debug,
{
    /// Creates a new [Reflector] using the specified API, writer, and watcher config.
    ///
    /// The function awaits until the cache is fully ready to serve objects.
    /// Returns a `Result` with either the initialized [Reflector] or an error.
    async fn try_new(
        api: Api<K>,
        writer: Writer<K>,
        wc: watcher::Config,
    ) -> Result<Self, K8sError> {
        let reader = writer.as_reader();
        let writer_close_handle = Self::start_reflector(api, wc, writer).abort_handle();

        reader.wait_until_ready().await?; // TODO: should we implement a timeout?

        Ok(Reflector {
            reader,
            writer_close_handle,
        })
    }

    /// Returns a clone of the internal store reader to access the cached Kubernetes objects.
    pub fn reader(&self) -> reflector::Store<K> {
        self.reader.clone()
    }

    /// Spawns a tokio task waiting for events and updating the provided writer.
    /// Returns the task [JoinHandle<()>].
    fn start_reflector(api: Api<K>, wc: watcher::Config, writer: Writer<K>) -> JoinHandle<()> {
        tokio::spawn(async move {
            watcher(api, wc)
                // The watcher recovers automatically from api errors, the backoff could be customized.
                .default_backoff()
                // All changes are reflected into the writer.
                .reflect(writer)
                // We need to query the events to start the watcher.
                .touched_objects()
                .for_each(|o| {
                    if let Some(e) = o.err() {
                        warn!("Recoverable error watching k8s events: {}", e)
                    }
                    future::ready(())
                })
                .await // The watcher runs indefinitely.
        })
    }
}

impl<K> Drop for Reflector<K>
where
    K: kube::core::Resource + Clone + DeserializeOwned + Debug + Send + Sync + 'static,
    K::DynamicType: Eq + std::hash::Hash + Clone + Debug,
{
    /// When dropped, abort the writer task to ensure proper cleanup.
    fn drop(&mut self) {
        self.writer_close_handle.abort();
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use k8s_openapi::api::apps::v1::Deployment;
    use tokio::sync::oneshot::{channel, Sender};

    async fn mocked_writer_task(_send: Sender<()>) {
        // _send will be dropped when the task is finished
        loop {
            tokio::time::sleep(tokio::time::Duration::from_micros(10)).await;
        }
    }

    #[tokio::test]
    async fn test_reflector_abort_writer_on_drop() {
        // Create a writer and store using `()`, as `Deployment` has no dynamic type.
        let (_store, writer) = reflector::store::store::<Deployment>();
        let reader = writer.as_reader();

        let (send, recv) = channel();

        let reflector = Reflector {
            reader,
            writer_close_handle: tokio::spawn(mocked_writer_task(send)).abort_handle(),
        };

        // When the reflector is dropped, it should abort the `writer_task`. Consequently, the channel's receiver
        // finished with error <https://docs.rs/tokio/latest/tokio/sync/oneshot/error/struct.RecvError.html>.
        drop(reflector);

        assert!(recv.await.is_err());
    }
}
