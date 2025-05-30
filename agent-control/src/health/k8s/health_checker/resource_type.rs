use kube::api::TypeMeta;

use crate::agent_control::config::{helmrelease_v2_type_meta, instrumentation_v1beta1_type_meta};

pub enum ResourceType {
    HelmRelease,
    InstrumentationCRD,
}

pub struct UnsupportedResourceType;

impl TryFrom<&TypeMeta> for ResourceType {
    type Error = UnsupportedResourceType;

    fn try_from(value: &TypeMeta) -> Result<Self, Self::Error> {
        if value == &helmrelease_v2_type_meta() {
            Ok(ResourceType::HelmRelease)
        } else if value == &instrumentation_v1beta1_type_meta() {
            Ok(ResourceType::InstrumentationCRD)
        } else {
            Err(UnsupportedResourceType)
        }
    }
}
