use kube::api::TypeMeta;

use crate::super_agent::config::{helm_release_type_meta, instrumentation_type_meta};

pub enum ResourceType {
    HelmRelease,
    InstrumentationCRD,
}

pub struct UnsupportedResourceType;

impl TryFrom<&TypeMeta> for ResourceType {
    type Error = UnsupportedResourceType;

    fn try_from(value: &TypeMeta) -> Result<Self, Self::Error> {
        if value == &helm_release_type_meta() {
            Ok(ResourceType::HelmRelease)
        } else if value == &instrumentation_type_meta() {
            Ok(ResourceType::InstrumentationCRD)
        } else {
            Err(UnsupportedResourceType)
        }
    }
}
