use super::super::error::K8sError;
use futures::StreamExt;
use kube::{
    Api, Client,
    core::DynamicObject,
    discovery::ApiResource,
    runtime::{
        WatchStreamExt,
        reflector::{self, Store, store::Writer},
        watcher,
    },
};
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::{fmt::Debug, future::Future, time::Duration};
use tokio::task::{AbortHandle, JoinHandle};
use tracing::{debug, error, trace, warn};

const REFLECTOR_START_TIMEOUT: Duration = Duration::from_secs(10);
const REFLECTOR_START_MAX_ATTEMPTS: u32 = 3;

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
    /// # `stop_on_watcher_err` - If true, the reflector will stop when the watcher fails.
    ///
    /// # Returns
    /// Returns the newly built reflector or an error.
    pub async fn try_build_with_api_resource(
        &self,
        ns: &str,
        api_resource: &ApiResource,
        stop_on_watcher_err: bool,
    ) -> Result<Reflector<DynamicObject>, K8sError> {
        trace!("Building k8s reflector for {:?}", api_resource);
        Reflector::retry_build_on_timeout(REFLECTOR_START_MAX_ATTEMPTS, || async {
            Reflector::try_new(
                Api::namespaced_with(self.client.clone(), ns, api_resource),
                self.watcher_config(),
                REFLECTOR_START_TIMEOUT,
                || Writer::new(api_resource.clone()),
                stop_on_watcher_err,
            )
            .await
        })
        .await
        .inspect_err(|err| error!(%err, "Failure building reflector for {:?}", api_resource))
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
        watcher_config: watcher::Config,
        start_timeout: Duration,
        writer_builder_fn: impl Fn() -> Writer<K>,
        stop_on_watcher_err: bool,
    ) -> Result<Self, K8sError> {
        let writer = writer_builder_fn();
        let reader = writer.as_reader();
        let writer_close_handle =
            Self::start_reflector(api, watcher_config, writer, stop_on_watcher_err).abort_handle();

        Self::wait_until_reader_is_ready(&reader, start_timeout).await?;
        Ok(Reflector {
            reader,
            writer_close_handle,
        })
    }

    /// Retries the provided `build_fn` if it fails with a timeout error until it stops timing out or `max_attempts`
    /// is reached.
    async fn retry_build_on_timeout<Fut>(
        max_attempts: u32,
        build_fn: impl Fn() -> Fut,
    ) -> Result<Self, K8sError>
    where
        Fut: Future<Output = Result<Self, K8sError>>,
    {
        for attempt in 1..=max_attempts {
            match build_fn().await {
                Err(K8sError::ReflectorTimeout(err)) => {
                    debug!("Reflector build timed-out: {err} (Attempt {attempt}/{max_attempts})",);
                    continue;
                }
                Err(err) => {
                    return Err(err);
                }
                Ok(reflector) => {
                    return Ok(reflector);
                }
            }
        }
        Err(K8sError::ReflectorTimeout(format!(
            "reflector build timed-out after {max_attempts} attempts"
        )))
    }

    pub fn list(&self) -> Vec<Arc<K>> {
        self.reader.state()
    }

    pub fn is_running(&self) -> bool {
        !self.writer_close_handle.is_finished()
    }

    /// Spawns a tokio task waiting for events and updating the provided writer.
    /// Returns the task [JoinHandle<()>].
    fn start_reflector(
        api: Api<K>,
        wc: watcher::Config,
        writer: Writer<K>,
        stop_on_watcher_err: bool,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let resource_url = api.resource_url().to_string();

            let mut stream = watcher(api, wc)
                // The watcher recovers automatically from api errors, the backoff could be customized.
                .default_backoff()
                // All changes are reflected into the writer.
                .reflect(writer)
                // We need to query the events to start the watcher.
                .touched_objects()
                .boxed();

            loop {
                match stream.next().await {
                    // On some cases like after removing the CRD watched by the reflector, the watcher will fail.
                    // In those particular cases, we should stop the reflector, to avoid serving outdated stored data.
                    // As is not complealty defined which are exactly those cases, the approach taken is to stop the current
                    // reflector assuming that a new one will be created with correct data.
                    Some(Err(watcher::Error::WatchFailed(err))) if stop_on_watcher_err => {
                        warn!(
                            "Error updating internal cache for resource '{}'. The cache will attempt to auto-recover: {}",
                            resource_url, err
                        );
                        break;
                    }
                    Some(Err(e)) => {
                        debug!("Recoverable error watching k8s events: {}", e)
                    }
                    _ => {}
                }
            }
        })
    }

    /// Waits until the reflector's reader is ready with the provided timeout.
    async fn wait_until_reader_is_ready(
        reader: &Store<K>,
        timeout: Duration,
    ) -> Result<(), K8sError> {
        Ok(tokio::time::timeout(timeout, reader.wait_until_ready())
            .await
            .map_err(|_| {
                K8sError::ReflectorTimeout(format!("reader not ready after {:?}", timeout))
            })??)
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
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use k8s_openapi::api::apps::v1::Deployment;
    use std::sync::Arc;
    use tokio::sync::{mpsc, oneshot};

    async fn mocked_writer_task(_send: oneshot::Sender<()>) {
        // _send will be dropped when the task is finished
        loop {
            tokio::time::sleep(tokio::time::Duration::from_micros(10)).await;
        }
    }

    fn reflector() -> Reflector<Deployment> {
        let (reader, _) = reflector::store::store::<Deployment>();
        Reflector {
            reader,
            writer_close_handle: tokio::spawn(async {}).abort_handle(),
        }
    }

    #[tokio::test]
    async fn test_reflector_abort_writer_on_drop() {
        // Create a writer and store using `()`, as `Deployment` has no dynamic type.
        let (_store, writer) = reflector::store::store::<Deployment>();
        let reader = writer.as_reader();

        let (send, recv) = oneshot::channel();

        let reflector = Reflector {
            reader,
            writer_close_handle: tokio::spawn(mocked_writer_task(send)).abort_handle(),
        };

        // When the reflector is dropped, it should abort the `writer_task`. Consequently, the channel's receiver
        // finished with error <https://docs.rs/tokio/latest/tokio/sync/oneshot/error/struct.RecvError.html>.
        drop(reflector);

        assert!(recv.await.is_err());
    }

    #[tokio::test]
    async fn test_reflector_wait_for_reader_reflector_error() {
        let (_store, writer) = reflector::store::store::<Deployment>();
        let reader = writer.as_reader();
        drop(writer); // dropping the writer will make the reader fail
        let timeout = Duration::from_millis(50);
        let result = Reflector::wait_until_reader_is_ready(&reader, timeout).await;
        assert_matches!(result.unwrap_err(), K8sError::ReflectorWriterDropped(_));
    }

    #[tokio::test]
    async fn test_reflector_wait_for_reader_timeout() {
        let (_store, writer) = reflector::store::store::<Deployment>();
        let reader = writer.as_reader();
        let timeout = Duration::from_millis(50);
        let result = Reflector::wait_until_reader_is_ready(&reader, timeout).await;
        assert_matches!(result.unwrap_err(), K8sError::ReflectorTimeout(s) => {
            s.contains(format!("{:?}", timeout).as_str());
        });
    }

    #[tokio::test]
    async fn test_reflector_wait_for_reader_ok() {
        let (_store, mut writer) = reflector::store::store::<Deployment>();
        let reader = writer.as_reader();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            writer.apply_watcher_event(&watcher::Event::InitDone); // Event sent when the watcher is initialized
        });
        let timeout = Duration::from_millis(500);
        let result = Reflector::wait_until_reader_is_ready(&reader, timeout).await;
        assert!(
            result.is_ok(),
            "Expected ok, got error {:?}",
            result.unwrap_err()
        );
    }

    #[tokio::test]
    async fn test_reflector_retry_on_timeout_fail_when_max_attempts_are_reached() {
        let (sender, receiver) = mpsc::channel(10);
        let sender = Arc::new(sender);

        // mock builder which will always fail with timeout
        async fn always_timeout_builder(
            s: Arc<mpsc::Sender<()>>,
        ) -> Result<Reflector<Deployment>, K8sError> {
            let _ = s.send(()).await;
            Err(K8sError::ReflectorTimeout("timeout".to_string()))
        }

        let max_attempts = 5;
        let result = Reflector::<Deployment>::retry_build_on_timeout(max_attempts, || {
            always_timeout_builder(sender.clone())
        })
        .await;

        assert_matches!(result, Err(K8sError::ReflectorTimeout(_)));
        assert_eq!(
            receiver.len(),
            max_attempts as usize,
            "The builder is expected to be called {} times",
            max_attempts
        )
    }

    #[tokio::test]
    async fn test_reflector_retry_on_timeout_do_not_retry_if_ok() {
        let (sender, receiver) = mpsc::channel(10);
        let sender = Arc::new(sender);

        // The builder always succeeds
        async fn always_success_builder(
            s: Arc<mpsc::Sender<()>>,
        ) -> Result<Reflector<Deployment>, K8sError> {
            let _ = s.send(()).await;
            Ok(reflector())
        }

        let max_attempts = 5;
        let result = Reflector::<Deployment>::retry_build_on_timeout(max_attempts, || {
            always_success_builder(sender.clone())
        })
        .await;

        assert!(
            result.is_ok(),
            "Expected ok, got error {:?}",
            result.unwrap_err()
        );
        assert_eq!(
            receiver.len(),
            1,
            "The reflector is expected to be called only once",
        )
    }

    #[tokio::test]
    async fn test_reflector_retry_on_timeout_other_error() {
        let (sender, receiver) = mpsc::channel(10);
        let sender = Arc::new(sender);

        async fn always_fail_builder(
            s: Arc<mpsc::Sender<()>>,
        ) -> Result<Reflector<Deployment>, K8sError> {
            let _ = s.send(()).await;
            Err(K8sError::ReflectorsNotInitialized)
        }

        let max_attempts = 5;
        let result = Reflector::<Deployment>::retry_build_on_timeout(max_attempts, || {
            always_fail_builder(sender.clone())
        })
        .await;

        assert_matches!(result, Err(K8sError::ReflectorsNotInitialized));
        assert_eq!(
            receiver.len(),
            1,
            "The reflector is expected to be called only once",
        )
    }

    #[tokio::test]
    async fn test_reflector_retry_on_timeout_failure_and_then_success() {
        let (sender, receiver) = mpsc::channel(10);
        let (sender, receiver) = (Arc::new(sender), Arc::new(receiver));

        async fn fail_and_then_success(
            sender: Arc<mpsc::Sender<()>>,
            receiver: Arc<mpsc::Receiver<()>>,
        ) -> Result<Reflector<Deployment>, K8sError> {
            let _ = sender.send(()).await;
            // The first attempt should time-out
            if receiver.len() == 1 {
                Err::<Reflector<Deployment>, K8sError>(K8sError::ReflectorTimeout(
                    "timeout".to_string(),
                ))
            } else {
                Ok(reflector())
            }
        }

        let max_attempts = 10;
        let result = Reflector::<Deployment>::retry_build_on_timeout(max_attempts, || {
            fail_and_then_success(sender.clone(), receiver.clone())
        })
        .await;

        assert!(
            result.is_ok(),
            "Expected ok, got error {:?}",
            result.unwrap_err()
        );
        assert_eq!(
            receiver.len(),
            2,
            "The builder is expected to be called twice"
        )
    }
}
