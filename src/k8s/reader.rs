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

pub struct ReflectorBuilder {
    client: Client,
    namespace: String,
    field_selector: Option<String>,
    label_selector: Option<String>,
}

impl ReflectorBuilder {
    /// Returns a reflector builder, consuming both provided the client and the namespace.
    pub fn new(client: Client, namespace: String) -> Self {
        ReflectorBuilder {
            client,
            namespace,
            field_selector: None,
            label_selector: None,
        }
    }
    pub fn with_fields(mut self, field_selector: String) -> Self {
        self.field_selector = Some(field_selector);
        self
    }

    pub fn with_labels(mut self, labels: String) -> Self {
        self.label_selector = Some(labels);
        self
    }

    /// Builds the DynamicObject reflector using the builder.
    pub async fn dynamic_object_reflector(
        &self,
        api_resource: &ApiResource,
    ) -> Result<DynamicObjectReflector, K8sError> {
        let api: Api<DynamicObject> =
            Api::namespaced_with(self.client.to_owned(), &self.namespace, api_resource);

        let writer: Writer<DynamicObject> = reflector::store::Writer::new(api_resource.to_owned());

        let wc = watcher::Config {
            label_selector: self.label_selector.clone(),
            field_selector: self.field_selector.clone(),
            ..Default::default()
        };

        DynamicObjectReflector::new(api, writer, wc).await
    }
}

pub struct DynamicObjectReflector {
    reader: reflector::Store<DynamicObject>,
    writer_task: AbortHandle,
}

impl DynamicObjectReflector {
    async fn new(
        api: Api<DynamicObject>,
        writer: Writer<DynamicObject>,
        wc: watcher::Config,
    ) -> Result<Self, K8sError> {
        let reader = writer.as_reader();
        let writer_task = Self::start_reflector(writer, api, wc).abort_handle();

        reader.wait_until_ready().await?;

        Ok(DynamicObjectReflector {
            reader,
            writer_task,
        })
    }

    fn start_reflector(
        writer: Writer<DynamicObject>,
        api: Api<DynamicObject>,
        wc: watcher::Config,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            watcher(api, wc)
                .default_backoff()
                .reflect(writer)
                .touched_objects()
                .for_each(|o| {
                    if let Some(e) = o.err() {
                        warn!("Recoverable error watching k8s events: {}", e)
                    }
                    future::ready(())
                })
                .await
        })
    }

    // A copy of the reader
    // TODO: we are cloning it for now, but we need to check what's the best approach considering its usage.
    pub fn reader(&self) -> reflector::Store<DynamicObject> {
        self.reader.clone()
    }
}

impl Drop for DynamicObjectReflector {
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
        let writer = reflector::store::Writer::new(ApiResource::from_gvk(&gvk));
        let reader = writer.as_reader();
        let (send, recv) = channel();
        let reflector = DynamicObjectReflector {
            reader,
            writer_task: tokio::spawn(mocked_writer_task(send)).abort_handle(),
        };
        drop(reflector);
        assert!(recv.await.is_err()); // must get err because the sender is dropped.
    }

    #[tokio::test]
    // The client's requires async
    async fn test_reflector_builder_options() {
        let (mock_service, _) = tower_test::mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");

        let builder = ReflectorBuilder::new(client, "builder-ns".into())
            .with_fields("field1==v1,field2!=value2".into())
            .with_labels("prometheus.io/scrape=true".into());

        assert_eq!(builder.namespace, "builder-ns".to_string());

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
