use futures::StreamExt;
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
use tokio::task::{AbortHandle, JoinHandle};
use tracing::warn;

use super::error::K8sError;

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
    field_selector: Option<String>,
    label_selector: Option<String>,
}

impl ReflectorBuilder {
    /// Returns a reflector builder, consuming both the provided client and the namespace.
    pub fn new(client: Client) -> Self {
        ReflectorBuilder {
            client,
            field_selector: None,
            label_selector: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_fields(mut self, field_selector: String) -> Self {
        self.field_selector = Some(field_selector);
        self
    }

    #[allow(dead_code)]
    pub fn with_labels(mut self, labels: String) -> Self {
        self.label_selector = Some(labels);
        self
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
            label_selector: self.label_selector.clone(),
            field_selector: self.field_selector.clone(),
            ..Default::default()
        };

        DynamicObjectReflector::new(api, writer, wc).await
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
    writer_task: AbortHandle,
}

impl DynamicObjectReflector {
    /// Creates a DynamicObjectReflector (used by [ReflectorBuilder]).
    async fn new(
        api: Api<DynamicObject>,
        writer: Writer<DynamicObject>,
        wc: watcher::Config,
    ) -> Result<Self, K8sError> {
        let reader = writer.as_reader();
        let writer_task = Self::start_reflector(writer, api, wc).abort_handle();

        reader.wait_until_ready().await?; // TODO: should we implement a timeout?

        Ok(DynamicObjectReflector {
            reader,
            writer_task,
        })
    }

    /// Starts a new async routine which executes a watcher that reflects the event changes in the
    /// provided writer and write logs on event failures.
    fn start_reflector(
        writer: Writer<DynamicObject>,
        api: Api<DynamicObject>,
        wc: watcher::Config,
    ) -> JoinHandle<()> {
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
        self.writer_task.abort();
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use http::{Request, Response};
    use hyper::Body;
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
            writer_task: tokio::spawn(mocked_writer_task(send)).abort_handle(),
        };
        // When the reflector is dropped, it should abort the `writer_task`. Consequently, the channel's receiver
        // finished with error <https://docs.rs/tokio/latest/tokio/sync/oneshot/error/struct.RecvError.html>.
        drop(reflector);
        assert!(recv.await.is_err());
    }

    #[tokio::test]
    // The client's mock requires an async
    async fn test_reflector_builder_options() {
        let (mock_service, _) = tower_test::mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "builder-ns");

        let builder = ReflectorBuilder::new(client)
            .with_fields("field1==v1,field2!=value2".into())
            .with_labels("prometheus.io/scrape=true".into());

        assert_eq!(builder.client.default_namespace(), "builder-ns".to_string());

        assert_eq!(
            builder.field_selector,
            Some("field1==v1,field2!=value2".to_string())
        );

        assert_eq!(
            builder.label_selector,
            Some("prometheus.io/scrape=true".to_string())
        );
    }
}
