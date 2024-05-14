use futures::StreamExt;
use std::fmt::Debug;
use std::future;

use k8s_openapi::{Metadata, NamespaceResourceScope, Resource};

use kube::{
    core::{DynamicObject, ObjectMeta},
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

use super::error::K8sError;

/// The `ResourceWithReflector` trait represents Kubernetes resources that have a namespace scope.
/// It includes metadata and traits required for Kubernetes object reflection and caching.
///
/// # Type Parameters
///   - Implement the `Resource` trait with a `NamespaceResourceScope`.
///   - Have a static `ObjectMeta` metadata type through `Metadata`.
///   - Be capable of being deserialized via `DeserializeOwned`.
///   - Be clonable, debuggable, and thread-safe (`Send` and `Sync`).
///
/// By implementing this trait for various Kubernetes resources like DaemonSet, Deployment, ReplicaSet,
/// and StatefulSet, we ensure that they can be managed using a generic pattern.
pub trait ResourceWithReflector:
    Resource<Scope = NamespaceResourceScope>
    + Clone
    + DeserializeOwned
    + Debug
    + Metadata<Ty = ObjectMeta>
    + Send
    + Sync
    + 'static
{
}

/// Reflector builder holds the arguments to build a reflector. Its implementation allows creating a reflector.
///
/// ## Example:
/// ```ignore
/// // It depends on `kube::Client` and `kube::discovery::ApiResource`
/// let dynamic_object_reflector = reader::ReflectorBuilder::new(client, "namespace")
///     .with_labels("key=value")
///     .dynamic_object_reflector(api_resource);
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
    pub async fn dynamic_object_reflector(
        &self,
        api_resource: &ApiResource,
    ) -> Result<DynamicObjectReflector, K8sError> {
        // The api consumes the client, so it needs to be owned to allow sharing the builder.
        let api: Api<DynamicObject> =
            Api::default_namespaced_with(self.client.to_owned(), api_resource);

        let writer: Writer<DynamicObject> = reflector::store::Writer::new(api_resource.to_owned());

        // Selectors need to be cloned since the builder could create more reflectors.
        let wc = watcher::Config {
            ..Default::default()
        };

        DynamicObjectReflector::new(api, writer, wc).await
    }

    /// Builds a generic resource reflector using the builder.
    ///
    /// # Type Parameters
    /// * `K` - Kubernetes resource type implementing the required traits.
    ///
    /// # Arguments
    /// * `namespace` - Namespace in which the resource is located.
    ///
    /// # Returns
    /// Returns the newly built generic reflector or an error.
    pub async fn try_new_resource_reflector<K>(
        &self,
        namespace: &str,
    ) -> Result<K8sControllerReflector<K>, K8sError>
    where
        K: ResourceWithReflector,
    {
        // Create an API instance for the resource type
        let api: Api<K> = Api::namespaced(self.client.clone(), namespace);

        // Initialize the writer for the resource type
        let writer: Writer<K> = reflector::store::Writer::default();

        // Clone field and label selectors
        let wc = watcher::Config {
            ..Default::default()
        };

        // Create and return the generic resource reflector
        K8sControllerReflector::new(api, writer, wc).await
    }
}

/// DynamicObjectReflector wraps kube-rs reflectors using any kubernetes object (DynamicObject).
/// The reflector consists of a writer (`reflector::store::Writer<K>`) which reflects all k8s events by means of
/// a watcher, and readers (`reflector::store::Store<K>`) which provide an efficient way to query the corresponding
/// objects.
/// It starts a new routine to start the watcher and keep the reader updated which will be stopped on reflector's drop,
/// and it also holds a reference to a reader which can be safely cloned during the reflector's lifetime.
pub struct DynamicObjectReflector {
    reader: reflector::Store<DynamicObject>,
    writer_close_handle: AbortHandle,
}

impl DynamicObjectReflector {
    /// Creates a DynamicObjectReflector (used by [ReflectorBuilder]).
    async fn new(
        api: Api<DynamicObject>,
        writer: Writer<DynamicObject>,
        wc: watcher::Config,
    ) -> Result<Self, K8sError> {
        let reader = writer.as_reader();
        let writer_close_handle = start_reflector(api, wc, writer).abort_handle();

        reader.wait_until_ready().await?; // TODO: should we implement a timeout?

        Ok(DynamicObjectReflector {
            reader,
            writer_close_handle,
        })
    }

    /// Returns a copy of the reader.
    // TODO: we are cloning it for now, but we need to check what's the best approach considering its usage.
    // We may include additional methods using the reader instead of exposing a copy.
    pub fn reader(&self) -> reflector::Store<DynamicObject> {
        self.reader.clone()
    }
}

impl Drop for DynamicObjectReflector {
    // Abort the reflector's writer task when it drops.
    fn drop(&mut self) {
        self.writer_close_handle.abort();
    }
}

fn start_reflector<K>(api: Api<K>, wc: watcher::Config, writer: Writer<K>) -> JoinHandle<()>
where
    K: kube::core::Resource + Clone + DeserializeOwned + Debug + Send + Sync + 'static,
    K::DynamicType: Eq + std::hash::Hash + Clone,
{
    //let writer = writer_builder();
    tokio::spawn(async move {
        watcher(api, wc)
            // The watcher recovers automatically from api errores, the backoff could be customized.
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

/// A generic Kubernetes reflector for resources that implement the `k8s_openapi::Resource` trait and have a namespace scope.
/// The `K8sControllerReflector` works by keeping an internal reader-writer pair:
/// - The reader keeps a read-only cache of Kubernetes objects.
/// - The writer continuously updates the cache based on the API stream.
///
/// The writer's async task is aborted when the reflector is dropped.
pub struct K8sControllerReflector<K>
where
    K: ResourceWithReflector,
{
    /// The read-only store that maintains a cache of Kubernetes objects of type `K`.
    reader: reflector::Store<K>,
    /// Handle for the writer task, which updates the cache. Used to abort the task on drop.
    writer_close_handle: AbortHandle,
}

impl<K> K8sControllerReflector<K>
where
    K: ResourceWithReflector,
{
    /// Creates a new `K8sControllerReflector` using the specified API, writer, and watcher config.
    ///
    /// The function awaits until the cache is fully ready to serve objects.
    /// Returns a `Result` with either the initialized `K8sControllerReflector` or an error.
    async fn new(api: Api<K>, writer: Writer<K>, wc: watcher::Config) -> Result<Self, K8sError> {
        let reader = writer.as_reader();
        let writer_close_handle = start_reflector(api, wc, writer).abort_handle();

        reader.wait_until_ready().await?; // TODO: should we implement a timeout?

        Ok(K8sControllerReflector {
            reader,
            writer_close_handle,
        })
    }

    /// Returns a clone of the internal store reader to access the cached Kubernetes objects.
    pub fn reader(&self) -> reflector::Store<K> {
        self.reader.clone()
    }
}

impl<K> Drop for K8sControllerReflector<K>
where
    K: ResourceWithReflector,
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
    use kube::core::GroupVersionKind;
    use tokio::sync::oneshot::{channel, Sender};

    async fn mocked_writer_task(_send: Sender<()>) {
        // _send will be dropped when the task is finished
        loop {
            tokio::time::sleep(tokio::time::Duration::from_micros(10)).await;
        }
    }

    #[tokio::test]
    async fn test_reflector_abort_writer_on_drop() {
        let gvk = GroupVersionKind::gvk("test.group", "v1", "TestKind");
        // Mocked reader and writer
        let writer = reflector::store::Writer::new(ApiResource::from_gvk(&gvk));
        let reader = writer.as_reader();
        let (send, recv) = channel();
        let reflector = DynamicObjectReflector {
            reader,
            writer_close_handle: tokio::spawn(mocked_writer_task(send)).abort_handle(),
        };
        // When the reflector is dropped, it should abort the `writer_task`. Consequently, the channel's receiver
        // finished with error <https://docs.rs/tokio/latest/tokio/sync/oneshot/error/struct.RecvError.html>.
        drop(reflector);
        assert!(recv.await.is_err());
    }

    #[tokio::test]
    async fn test_generic_reflector_abort_writer_on_drop() {
        // Create a writer and store using `()`, as `Deployment` has no dynamic type.
        let (_store, writer) = reflector::store::store::<Deployment>();
        let reader = writer.as_reader();

        let (send, recv) = channel();

        let reflector = K8sControllerReflector {
            reader,
            writer_close_handle: tokio::spawn(mocked_writer_task(send)).abort_handle(),
        };

        // When the reflector is dropped, it should abort the `writer_task`. Consequently, the channel's receiver
        // finished with error <https://docs.rs/tokio/latest/tokio/sync/oneshot/error/struct.RecvError.html>.
        drop(reflector);

        assert!(recv.await.is_err());
    }
}
