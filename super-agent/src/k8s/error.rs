use crate::super_agent::config::{AgentTypeError, SuperAgentConfigError};
use kube::core::gvk::ParseGroupVersionError;
use kube::{api, config::KubeconfigError};

#[derive(thiserror::Error, Debug)]
pub enum K8sError {
    #[error("it is not possible to create a k8s client: {0}")]
    UnableToSetupClient(String),

    #[error("the kube client returned an error: `{0}`")]
    Generic(#[from] kube::Error),

    #[error("it is not possible to read kubeconfig: `{0}`")]
    UnableToSetupClientKubeconfig(#[from] KubeconfigError),

    #[error("cannot start a k8s reader `{0}`")]
    ReflectorWriterDropped(#[from] kube::runtime::reflector::store::WriterDropped),

    #[error("cannot post object `{0}`")]
    CommitError(#[from] api::entry::CommitError),

    #[error("the kind of the cr is missing")]
    MissingCRKind,

    #[error("the name of the cr is missing")]
    MissingCRName,

    #[error("{0} does not have .metadata.name")]
    MissingName(String),

    #[error("error parsing GroupVersion: `{0}`")]
    ParseGroupVersion(#[from] ParseGroupVersionError),

    #[error("while getting dynamic resource: {0}")]
    GetDynamic(String),

    #[error("failed to parse yaml: {0}")]
    FailedToParseYaml(#[from] serde_yaml::Error),

    #[error("reflectors not initialized")]
    ReflectorsNotInitialized,

    #[error("reflector timeout: {0}")]
    ReflectorTimeout(String),

    #[error("api resource kind not present in the cluster: {0}")]
    MissingAPIResource(String),
}

#[derive(thiserror::Error, Debug)]
pub enum GarbageCollectorK8sError {
    #[error("the kube client returned an error: `{0}`")]
    Generic(#[from] K8sError),

    #[error("garbage collector failed loading config store: `{0}`")]
    LoadingConfigStore(#[from] SuperAgentConfigError),

    #[error("garbage collector executed with empty current agents list")]
    MissingActiveAgents(),

    #[error("garbage collector fetched resources without required labels")]
    MissingLabels(),

    #[error("garbage collector fetched resources without required annotations")]
    MissingAnnotations(),

    #[error("unable to parse AgentType: `{0}`")]
    ParsingAgentType(#[from] AgentTypeError),
}
