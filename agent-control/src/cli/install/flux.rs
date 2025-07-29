use std::sync::Arc;

use kube::api::DynamicObject;

use crate::cli::install::{DynamicObjectListBuilder, InstallData};

/// Implementation of [`DynamicObjectListBuilder`] for generating the dynamic object lists corresponding to the Agent Control resources.
///
/// To be applied via [`install_or_upgrade`](super::install_or_upgrade).
pub struct InstallFlux;

impl DynamicObjectListBuilder for InstallFlux {
    fn build_dynamic_object_list(
        &self,
        namespace: &str,
        maybe_existing_helm_release: Option<Arc<DynamicObject>>,
        data: &InstallData,
    ) -> Vec<kube::api::DynamicObject> {
        todo!()
    }
}
