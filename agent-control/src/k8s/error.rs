use crate::agent_control::agent_id::AgentIDError;
use crate::agent_control::config::AgentControlConfigError;
use crate::agent_type::agent_type_id::AgentTypeIDError;
use kube::core::gvk::ParseGroupVersionError;
use kube::{api, config::KubeconfigError};

#[derive(thiserror::Error, Debug)]
pub enum K8sError {
    #[error("the kube client returned an error: `{0}`")]
    Generic(#[from] kube::Error),

    #[error("it is not possible to read kubeconfig: `{0}`")]
    UnableToSetupClientKubeconfig(#[from] KubeconfigError),

    #[error("cannot start a k8s reader `{0}`")]
    ReflectorWriterDropped(#[from] kube::runtime::reflector::store::WriterDropped),

    // We need to add the debug info since the string representation of CommitError hide the source of the error
    #[error("cannot post object `{0:?}`")]
    CommitError(#[from] api::entry::CommitError),

    #[error("cannot patch object {0} with `{0}`")]
    PatchError(String, String),

    #[error("the kind of the cr is missing")]
    MissingCRKind,

    #[error("the name of the cr is missing")]
    MissingCRName,

    #[error("the MissingCRNamespace of the cr is missing")]
    MissingCRNamespace,

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
    LoadingConfigStore(#[from] AgentControlConfigError),

    #[error("garbage collector executed with empty current agents list")]
    MissingActiveAgents(),

    #[error("garbage collector fetched resources without required labels")]
    MissingLabels(),

    #[error("garbage collector fetched resources without required annotations")]
    MissingAnnotations(),

    #[error("unable to parse AgentTypeID: `{0}`")]
    ParsingAgentType(#[from] AgentTypeIDError),

    #[error("unable to parse AgentID: `{0}`")]
    ParsingAgentId(#[from] AgentIDError),
}
