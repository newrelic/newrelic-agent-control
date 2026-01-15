use crate::agent_control::config::instrumentation_v1beta3_type_meta;
use crate::agent_type::guid_config::{GuidCheckerInitialDelay, GuidCheckerInterval};
use crate::checkers::guid::k8s::resources::instrumentation::K8sGuidInstrumentation;
use crate::checkers::guid::{EntityGuid, GuidCheckError, GuidChecker};
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
use kube::api::DynamicObject;
use std::fmt::Debug;
use std::sync::Arc;
use std::thread::sleep;
use tracing::{debug, info, info_span, warn};

pub const GUID_CHECKER_THREAD_NAME: &str = "guid_checker";

pub struct K8sGuidChecker {
    instrumentation_checker: K8sGuidInstrumentation,
}

impl K8sGuidChecker {
    pub fn new(
        k8s_client: Arc<SyncK8sClient>,
        resources: Arc<Vec<DynamicObject>>,
        opamp_field: String,
    ) -> Result<Option<Self>, GuidCheckError> {
        let target_type = instrumentation_v1beta3_type_meta();

        for resource in resources.iter() {
            let type_meta = get_type_meta(resource).map_err(|e| GuidCheckError(e.to_string()))?;

            if type_meta == target_type {
                let name = get_name(resource).map_err(|e| GuidCheckError(e.to_string()))?;
                let namespace =
                    get_namespace(resource).map_err(|e| GuidCheckError(e.to_string()))?;

                return Ok(Some(Self {
                    instrumentation_checker: K8sGuidInstrumentation::new(
                        k8s_client,
                        type_meta,
                        name,
                        namespace,
                        opamp_field,
                    ),
                }));
            }
        }

        Ok(None)
    }
}

impl GuidChecker for K8sGuidChecker {
    fn check_guid(&self) -> Result<EntityGuid, GuidCheckError> {
        self.instrumentation_checker.check_guid()
    }
}

pub(crate) fn spawn_guid_checker<S, T, F>(
    guid_checker_id: String,
    guid_checker: S,
    guid_event_publisher: EventPublisher<T>,
    guid_event_generator: F,
    interval: GuidCheckerInterval,
    initial_delay: GuidCheckerInitialDelay,
) -> StartedThreadContext
where
    S: GuidChecker + Send + Sync + 'static,
    T: Debug + Send + Sync + 'static,
    F: Fn(UpdateAttributesMessage) -> T + Send + Sync + 'static,
{
    let thread_name = format!("{guid_checker_id}_{GUID_CHECKER_THREAD_NAME}");
    let mut last_guid: Option<EntityGuid> = None;

    let callback = move |stop_consumer: EventConsumer<CancellationMessage>| loop {
        let span = info_span!(
            "guid_check",
            { ID_ATTRIBUTE_NAME } = %guid_checker_id
        );
        let _guard = span.enter();

        debug!("starting to check guid");
        sleep(initial_delay.into());

        match guid_checker.check_guid() {
            Ok(current_guid) => {
                let has_changed = match &last_guid {
                    Some(prev) => prev.guid != current_guid.guid,
                    None => true,
                };

                if has_changed {
                    info!("Agent guid changed to: {}", current_guid.guid);
                    publish_update_attributes_event(
                        &guid_event_publisher,
                        guid_event_generator(vec![Attribute::from((
                            AttributeType::Identifying,
                            current_guid.opamp_field.clone(),
                            current_guid.guid.clone(),
                        ))]),
                    );
                    last_guid = Some(current_guid);
                }
            }
            Err(error) => {
                warn!("failed to check agent guid: {error}");
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

    use crate::agent_control::defaults::APM_APPLICATION_ID;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::api::DynamicObject;
    use mockall::predicate::*;
    use serde_json::json;
    use std::sync::Arc;

    fn mock_client(guid_json: serde_json::Value) -> SyncK8sClient {
        let mut client = SyncK8sClient::new();
        let tm = instrumentation_v1beta3_type_meta();

        let obj = DynamicObject {
            types: Some(tm.clone()),
            metadata: ObjectMeta {
                name: Some("test-inst".into()),
                namespace: Some("default".into()),
                ..Default::default()
            },
            data: json!({ "status": guid_json }),
        };

        client
            .expect_get_dynamic_object()
            .with(eq(tm), eq("test-inst"), eq("default"))
            .returning(move |_, _, _| Ok(Some(Arc::new(obj.clone()))));

        client
    }

    fn create_input_resources() -> Arc<Vec<DynamicObject>> {
        let tm = instrumentation_v1beta3_type_meta();
        Arc::new(vec![DynamicObject {
            types: Some(tm),
            metadata: ObjectMeta {
                name: Some("test-inst".into()),
                namespace: Some("default".into()),
                ..Default::default()
            },
            data: json!({}),
        }])
    }

    #[test]
    fn test_guids_success_simple() {
        let guid = json!({ "entityGUIDs": ["GUID-123"] });
        let client = mock_client(guid);
        let resources = create_input_resources();

        let checker_wrapper =
            K8sGuidChecker::new(Arc::new(client), resources, APM_APPLICATION_ID.to_string())
                .unwrap()
                .expect("Should return a checker");

        let res = checker_wrapper.check_guid().unwrap();
        assert_eq!(res.guid, "GUID-123");
    }

    #[test]
    fn test_guids_error_mismatch() {
        let guid = json!({ "entityGUIDs": ["A", "B"] });
        let client = mock_client(guid);
        let resources = create_input_resources();

        let checker_wrapper =
            K8sGuidChecker::new(Arc::new(client), resources, APM_APPLICATION_ID.to_string())
                .unwrap()
                .expect("Should return a checker");

        let err = checker_wrapper.check_guid().unwrap_err();
        assert!(err.0.contains("Mismatching"));
    }
}
