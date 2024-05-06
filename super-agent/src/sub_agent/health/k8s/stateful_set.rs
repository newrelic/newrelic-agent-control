use crate::k8s::client::is_label_present;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy,
};
use crate::sub_agent::health::k8s::health_checker::LABEL_RELEASE_FLUX;
use k8s_openapi::api::apps::v1::{StatefulSet, StatefulSetSpec};
use std::sync::Arc;

/// Represents a health checker for the StatefulSets or a release.
pub struct K8sHealthStatefulSet {
    k8s_client: Arc<SyncK8sClient>,
    release_name: String,
    // TODO Waiting on https://github.com/kube-rs/kube/pull/1482, Hardcoding label for now
    // label_selector: String,
}

impl HealthChecker for K8sHealthStatefulSet {
    fn check_health(&self) -> Result<Health, HealthCheckerError> {
        let list_stateful_set = self.k8s_client.list_stateful_set().map_err(|err| {
            HealthCheckerError::new(format!(
                "Error fetching StatefulSets '{}': {}",
                &self.release_name, err
            ))
        })?;

        let release_name = self.release_name.as_str();
        let filtered_list_stateful_set = list_stateful_set
            .into_iter()
            .filter(|ss| is_label_present(&ss.metadata.labels, LABEL_RELEASE_FLUX, release_name));

        for ss in filtered_list_stateful_set {
            let ss_health = K8sHealthStatefulSet::check_health_single_stateful_set(ss)?;
            if !ss_health.is_healthy() {
                return Ok(ss_health);
            }
        }

        Ok(Healthy::default().into())
    }
}

impl K8sHealthStatefulSet {
    pub fn new(k8s_client: Arc<SyncK8sClient>, release_name: String) -> Self {
        Self {
            k8s_client,
            release_name,
        }
    }

    fn check_health_single_stateful_set(ss: StatefulSet) -> Result<Health, HealthCheckerError> {
        let name = ss.metadata.name.ok_or(HealthCheckerError::new(
            "StatefulSets without Name".to_string(),
        ))?;
        let spec = ss.spec.ok_or(HealthCheckerError::new(format!(
            "StatefulSets `{}` without Specs",
            name
        )))?;
        let status = ss.status.ok_or(HealthCheckerError::new(format!(
            "StatefulSets `{}` without Status",
            name
        )))?;

        // default partitions and replicas are respectively 0 and 1
        let (partition, replicas) = get_partition_and_replicas(spec);
        let expected_replicas = replicas - partition;

        if status.observed_generation != ss.metadata.generation {
            return Ok(Health::new_unhealthy_with_last_error(format!(
                "StatefulSets `{}` not ready: observed_generation not matching generation",
                name
            )));
        }

        if let Some(updated_replicas) = status.updated_replicas {
            if updated_replicas < expected_replicas {
                return Ok(Health::new_unhealthy_with_last_error(format!(
                        "StatefulSets `{}` not ready: updated_replicas `{}` fewer than expected_replicas `{}`",
                        name,
                        updated_replicas,
                        expected_replicas,
                    )));
            }
        }

        if let Some(ready_replicas) = status.ready_replicas {
            if replicas != ready_replicas {
                return Ok(Health::new_unhealthy_with_last_error(format!(
                    "StatefulSets `{}` not ready: replicas `{}` different from ready_replicas `{}`",
                    name, replicas, ready_replicas,
                )));
            }
        }

        if partition == 0 && status.current_revision != status.update_revision {
            return Ok(Health::new_unhealthy_with_last_error(format!(
                "StatefulSets `{}` not ready: current_revision not matching update_revision",
                name
            )));
        }

        Ok(Healthy::default().into())
    }
}

// Note that it is valid that no value is specified for replicas or partition
fn get_partition_and_replicas(spec: StatefulSetSpec) -> (i32, i32) {
    let mut partition = 0;
    let mut replicas = 1;

    if let Some(update_strategy) = spec.update_strategy {
        if let Some(rolling_update) = update_strategy.rolling_update {
            if let Some(p) = rolling_update.partition {
                partition = p;
            }
        }
    }

    if let Some(r) = spec.replicas {
        replicas = r;
    }

    (partition, replicas)
}
