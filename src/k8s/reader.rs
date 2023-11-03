use futures::StreamExt;
use std::future;

use kube::{
    core::{DynamicObject, GroupVersionKind},
    discovery,
    runtime::{
        reflector::{self, store::Writer},
        watcher, WatchStreamExt,
    },
    Api, Client, Resource,
};
use tokio::task::JoinHandle;
use tracing::warn;

use super::error::K8sError;

pub struct Builder {
    client: Client,
    namespace: String,
    gvk: GroupVersionKind,
    field_selector: Option<String>,
    label_selector: Option<String>,
}

impl Builder {
    pub fn with_fields(mut self, field_selector: String) -> Self {
        self.field_selector = Some(field_selector);
        self
    }

    pub fn with_labels(mut self, labels: String) -> Self {
        self.label_selector = Some(labels);
        self
    }

    pub async fn build(self) -> Result<Reader<DynamicObject>, K8sError> {
        let (api_resource, _) = discovery::pinned_kind(&self.client, &self.gvk).await?;
        let api: Api<DynamicObject> =
            Api::namespaced_with(self.client, &self.namespace, &api_resource);

        let writer: Writer<DynamicObject> = reflector::store::Writer::new(api_resource);
        let reader = writer.as_reader();

        let mut wc = watcher::Config::default();
        wc.label_selector = self.label_selector;
        wc.field_selector = self.field_selector;

        let writer_task = Reader::<DynamicObject>::start_reflector(writer, api, wc);

        reader.wait_until_ready().await?;

        Ok(Reader {
            reader,
            writer_task,
        })
    }
}

pub fn builder(client: Client, namespace: String, gvk: GroupVersionKind) -> Builder {
    Builder {
        client,
        namespace,
        gvk,
        field_selector: None,
        label_selector: None,
    }
}

pub struct Reader<K>
where
    K: 'static + Resource + Clone,
    K::DynamicType: Eq + std::hash::Hash,
{
    reader: reflector::Store<K>,
    writer_task: JoinHandle<()>,
}

impl<K> Reader<K>
where
    K: 'static + Resource + Clone,
    K::DynamicType: Eq + std::hash::Hash,
{
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
    // TODO: we are cloning it for now, but we need to check that's the best approach considering its usage.
    pub fn reader(&self) -> reflector::Store<K> {
        self.reader.clone()
    }
}

impl<K> Drop for Reader<K>
where
    K: 'static + Resource + Clone,
    K::DynamicType: Eq + std::hash::Hash,
{
    fn drop(&mut self) {
        self.writer_task.abort();
    }
}
