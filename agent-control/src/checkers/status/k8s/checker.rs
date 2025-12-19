use crate::agent_control::config::instrumentation_v1beta3_type_meta;
use crate::agent_type::version_config::{VersionCheckerInitialDelay, VersionCheckerInterval};
use crate::checkers::status::k8s::resources::instrumentation::K8sStatusInstrumentation;
use crate::checkers::status::{AgentStatus, StatusCheckError, StatusChecker};
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::{EventConsumer, EventPublisher};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::utils::{get_name, get_namespace, get_type_meta};
use crate::opamp::attributes::{
    Attribute, AttributeType, UpdateAttributesMessage, publish_update_attributes_event,
};
use crate::sub_agent::identity::ID_ATTRIBUTE_NAME;
use crate::utils::thread_context::{NotStartedThreadContext, StartedThreadContext};
use kube::api::{DynamicObject, TypeMeta};
use std::fmt::Debug;
use std::sync::Arc;
use std::thread::sleep;
use tracing::{debug, info, info_span, trace, warn};
use crate::agent_control::defaults::APM_APPLICATION_ID;

pub const STATUS_CHECKER_THREAD_NAME: &str = "status_checker";

#[derive(Debug)]
pub enum K8sResourceStatusChecker {
    NewRelic(K8sStatusInstrumentation),
}

impl StatusChecker for K8sResourceStatusChecker {
    fn check_status(&self) -> Result<AgentStatus, StatusCheckError> {
        match self {
            K8sResourceStatusChecker::NewRelic(c) => c.check_status(),
        }
    }
}

pub fn status_checkers_for_type_meta(
    type_meta: TypeMeta,
    k8s_client: Arc<SyncK8sClient>,
    name: String,
    namespace: String,
) -> Vec<K8sResourceStatusChecker> {
    if type_meta == instrumentation_v1beta3_type_meta() {
        vec![K8sResourceStatusChecker::NewRelic(
            K8sStatusInstrumentation::new(k8s_client, type_meta, name, namespace),
        )]
    } else {
        trace!("No status-checkers for TypeMeta {type_meta:?}");
        vec![]
    }
}

pub struct K8sStatusChecker<SC = K8sResourceStatusChecker>
where
    SC: StatusChecker,
{
    status_checkers: Vec<SC>,
}

impl K8sStatusChecker<K8sResourceStatusChecker> {
    pub fn new(
        k8s_client: Arc<SyncK8sClient>,
        resources: Arc<Vec<DynamicObject>>,
    ) -> Result<Option<Self>, StatusCheckError> {
        let mut status_checkers = vec![];

        for resource in resources.iter() {
            let type_meta = get_type_meta(resource).map_err(|e| StatusCheckError(e.to_string()))?;
            let name = get_name(resource).map_err(|e| StatusCheckError(e.to_string()))?;
            let namespace = get_namespace(resource).map_err(|e| StatusCheckError(e.to_string()))?;

            let resource_checkers =
                status_checkers_for_type_meta(type_meta, k8s_client.clone(), name, namespace);

            status_checkers.extend(resource_checkers);
        }

        if status_checkers.is_empty() {
            return Ok(None);
        }

        Ok(Some(Self { status_checkers }))
    }
}

impl<SC> StatusChecker for K8sStatusChecker<SC>
where
    SC: StatusChecker,
{
    fn check_status(&self) -> Result<AgentStatus, StatusCheckError> {
        let mut final_status = AgentStatus {
            status: "Unknown".to_string(),
            opamp_field: APM_APPLICATION_ID.to_string(),
        };

        for checker in &self.status_checkers {
            final_status = checker.check_status()?;
        }

        Ok(final_status)
    }
}

pub(crate) fn spawn_status_checker<S, T, F>(
    status_checker_id: String,
    status_checker: S,
    status_event_publisher: EventPublisher<T>,
    status_event_generator: F,
    interval: VersionCheckerInterval,
    initial_delay: VersionCheckerInitialDelay,
) -> StartedThreadContext
where
    S: StatusChecker + Send + Sync + 'static,
    T: Debug + Send + Sync + 'static,
    F: Fn(UpdateAttributesMessage) -> T + Send + Sync + 'static,
{
    let thread_name = format!("{status_checker_id}_{STATUS_CHECKER_THREAD_NAME}");

    let mut last_status: Option<AgentStatus> = None;

    let callback = move |stop_consumer: EventConsumer<CancellationMessage>| loop {
        let span = info_span!(
            "status_check",
            { ID_ATTRIBUTE_NAME } = %status_checker_id
        );
        let _guard = span.enter();

        debug!("starting to check status");

        sleep(initial_delay.into());

        match status_checker.check_status() {
            Ok(current_status) => {
                let has_changed = match &last_status {
                    Some(prev) => prev.status != current_status.status,
                    None => true,
                };

                if has_changed {
                    info!("Agent status/guid changed to: {}", current_status.status);
                    publish_update_attributes_event(
                        &status_event_publisher,
                        status_event_generator(vec![Attribute::from((
                            AttributeType::Identifying,
                            APM_APPLICATION_ID,
                            current_status.status.clone(),
                        ))]),
                    );
                    last_status = Some(current_status);
                }
            }
            Err(error) => {
                warn!("failed to check agent status: {error}");
            }
        }

        if stop_consumer.is_cancelled_with_timeout(interval.into()) {
            break;
        }
    };

    NotStartedThreadContext::new(thread_name, callback).start()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::config::instrumentation_v1beta3_type_meta;

    #[mockall_double::double]
    use crate::k8s::client::SyncK8sClient;

    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::api::DynamicObject;
    use mockall::predicate::*;
    use serde_json::json;
    use std::sync::Arc;

    fn mock_client(status_json: serde_json::Value) -> SyncK8sClient {
        let mut client = SyncK8sClient::new();

        let tm = instrumentation_v1beta3_type_meta();
        let obj = DynamicObject {
            types: Some(tm.clone()),
            metadata: ObjectMeta {
                name: Some("test-inst".into()),
                namespace: Some("default".into()),
                ..Default::default()
            },
            data: json!({ "status": status_json }),
        };

        client
            .expect_get_dynamic_object()
            .with(eq(tm), eq("test-inst"), eq("default"))
            .returning(move |_, _, _| Ok(Some(Arc::new(obj.clone()))));

        client
    }

    #[test]
    fn test_guids_success_single() {
        let status = json!({
            "entityGUIDs": ["GUID-123"]
        });

        let client = mock_client(status);

        let checker = K8sStatusInstrumentation::new(
            Arc::new(client),
            instrumentation_v1beta3_type_meta(),
            "test-inst".into(),
            "default".into(),
        );

        let res = checker.check_status().unwrap();
        assert_eq!(res.status, "GUID-123");
    }

    #[test]
    fn test_guids_success_multiple_identical() {
        let status = json!({
            "entityGUIDs": ["GUID-123", "GUID-123"]
        });
        let client = mock_client(status);
        let checker = K8sStatusInstrumentation::new(
            Arc::new(client),
            instrumentation_v1beta3_type_meta(),
            "test-inst".into(),
            "default".into(),
        );

        let res = checker.check_status().unwrap();
        assert_eq!(res.status, "GUID-123");
    }

    #[test]
    fn test_guids_error_empty() {
        let status = json!({ "entityGUIDs": [] });
        let client = mock_client(status);
        let checker = K8sStatusInstrumentation::new(
            Arc::new(client),
            instrumentation_v1beta3_type_meta(),
            "test-inst".into(),
            "default".into(),
        );

        let err = checker.check_status().unwrap_err();
        assert!(err.0.contains("empty"));
    }

    #[test]
    fn test_guids_error_mismatch() {
        let status = json!({ "entityGUIDs": ["A", "B"] });
        let client = mock_client(status);
        let checker = K8sStatusInstrumentation::new(
            Arc::new(client),
            instrumentation_v1beta3_type_meta(),
            "test-inst".into(),
            "default".into(),
        );

        let err = checker.check_status().unwrap_err();
        assert!(err.0.contains("Mismatching"));
    }
}
