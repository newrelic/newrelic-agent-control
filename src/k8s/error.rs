use kube::core::gvk::ParseGroupVersionError;
use kube::{api, config::KubeconfigError};

#[derive(thiserror::Error, Debug)]
pub enum K8sError {
    #[error("it is not possible to create a k8s client")]
    UnableToSetupClient,

    #[error("the kube client returned an error: `{0}`")]
    Generic(#[from] kube::Error),

    #[error("it is not possible to read kubeconfig: `{0}`")]
    UnableToSetupClientKubeconfig(#[from] KubeconfigError),

    #[error("cannot start a k8s reader `{0}`")]
    ReflectorWriterDropped(#[from] kube::runtime::reflector::store::WriterDropped),

    #[error("cannot post object `{0}`")]
    CommitError(#[from] api::entry::CommitError),

    #[error("unexpected resource definition: api_version: {0}, kind: {1}")]
    UnexpectedKind(String, String),

    #[error("error serializing/deserializing yaml: `{0}`")]
    SerdeYaml(#[from] serde_yaml::Error),

    #[error("the cm data is malformed")]
    CMMalformed(),

    #[error("the cm key is missing")]
    KeyIsMissing(),

    #[error("the kind of the cr is missing")]
    MissingKind(),

    #[error("the name of the cr is missing")]
    MissingName(),

    #[error("error parsing GroupVersion: `{0}`")]
    ParseGroupVersion(#[from] ParseGroupVersionError),
}
