use std::sync::Arc;

use kube::api::DynamicObject;

use crate::cli::install::{DynamicObjectListBuilder, InstallData};

/// Implementation of [`DynamicObjectListBuilder`] for generating the dynamic object lists corresponding to the Agent Control resources.
///
/// To be applied via [`install_or_upgrade`](super::install_or_upgrade).
pub struct InstallFlux;

pub const RELEASE_NAME: &str = "flux";

impl DynamicObjectListBuilder for InstallFlux {
    fn build_dynamic_object_list(
        &self,
        namespace: &str,
        maybe_existing_helm_release: Option<Arc<DynamicObject>>,
        data: &InstallData,
    ) -> Vec<kube::api::DynamicObject> {
        /*
        apiVersion: source.toolkit.fluxcd.io/v1
        kind: HelmRepository
        metadata:
          name: flux-repo
          namespace: default
        spec:
          interval: 1m
          url: https://fluxcd-community.github.io/helm-charts
        ---
        apiVersion: helm.toolkit.fluxcd.io/v2
        kind: HelmRelease
        metadata:
          name: flux2
        spec:
          interval: 1m
          chart:
            spec:
              sourceRef:
                kind: HelmRepository
                name: flux-repo
                namespace: default
              chart: flux2
              version: 2.15.0
          values:
            installCRDS: true
            sourceController:
              create: true
            helmController:
              create: true
            kustomizeController:
              create: false
            imageAutomationController:
              create: false
            imageReflectionController:
              create: false
            notificationController:
              create: false
        */
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_dynamic_object_list() {
        
    }
}
