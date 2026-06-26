//! Error types for the Kubernetes integration.
use crate::agent_control::agent_id::AgentIDError;
use crate::agent_control::config::AgentControlConfigError;
use crate::agent_type::agent_type_id::AgentTypeIDError;
use kube::core::gvk::ParseGroupVersionError;
use kube::{api, config::KubeconfigError};

/// Errors that can occur while performing Kubernetes operations.
#[derive(thiserror::Error, Debug)]
pub enum K8sError {
    /// A generic error described by the contained message.
    #[error("{0}")]
    Generic(String),

    /// The underlying kube client returned an error.
    #[error("the kube client returned an error: {0}")]
    KubeRs(Box<kube::Error>),

    /// The kubeconfig could not be read while setting up the client.
    #[error("it is not possible to read kubeconfig: {0}")]
    UnableToSetupClientKubeconfig(#[from] KubeconfigError),

    /// A reflector reader could not be started because its writer was dropped.
    #[error("cannot start a k8s reader {0}")]
    ReflectorWriterDropped(#[from] kube::runtime::reflector::store::WriterDropped),

    // We need to add the debug info since the string representation of CommitError hide the source of the error
    /// An object could not be posted to the cluster.
    #[error("cannot post object {0:?}")]
    CommitError(Box<api::entry::CommitError>),

    /// An object could not be patched: the name and patch are included.
    #[error("cannot patch object {0} with {1}")]
    PatchError(String, String),

    /// The resource kind is missing from the object.
    #[error("the kind of the resource is missing")]
    MissingResourceKind,

    /// The resource name is missing from the object.
    #[error("the name of the resource is missing")]
    MissingResourceName,

    /// The resource namespace is missing from the object.
    #[error("the namespace of the resource is missing")]
    MissingResourceNamespace,

    /// The named object has no `.metadata.name`.
    #[error("{0} does not have .metadata.name")]
    MissingName(String),

    /// A group/version string could not be parsed.
    #[error("error parsing GroupVersion: {0}")]
    ParseGroupVersion(#[from] ParseGroupVersionError),

    /// A dynamic resource could not be retrieved.
    #[error("while getting dynamic resource: {0}")]
    GetDynamic(String),

    /// A dynamic object could not be parsed into a concrete type.
    #[error("parsing dynamicObject into concrete Object: {0}, Kind: {1}")]
    ParseDynamic(String, String),

    /// A YAML document could not be parsed.
    #[error("failed to parse yaml: {0}")]
    FailedToParseYaml(#[from] serde_saphyr::Error),
    /// A YAML value could not be converted into the target type.
    #[error("failed to convert yaml value: {0}")]
    FailedToConvertValue(#[from] serde_json::Error),
    /// A value could not be serialized into YAML.
    #[error("failed to serialize yaml: {0}")]
    FailedToSerializeYaml(#[from] serde_saphyr::ser::Error), // codespell:ignore ser

    /// The reflectors have not been initialized yet.
    #[error("reflectors not initialized")]
    ReflectorsNotInitialized,

    /// A reflector timed out, with the contained reason.
    #[error("reflector timeout: {0}")]
    ReflectorTimeout(String),

    /// The requested API resource kind is not present in the cluster.
    #[error("api resource kind not present in the cluster: {0}")]
    MissingAPIResource(String),
}

impl From<kube::Error> for K8sError {
    fn from(err: kube::Error) -> Self {
        K8sError::KubeRs(Box::new(err))
    }
}

impl From<api::entry::CommitError> for K8sError {
    fn from(err: api::entry::CommitError) -> Self {
        K8sError::CommitError(Box::new(err))
    }
}

/// Errors that can occur while running the Kubernetes garbage collector.
#[derive(thiserror::Error, Debug)]
pub enum GarbageCollectorK8sError {
    /// A wrapped underlying [`K8sError`].
    #[error("the kube client returned an error: {0}")]
    Generic(#[from] K8sError),

    /// The config store could not be loaded.
    #[error("garbage collector failed loading config store: {0}")]
    LoadingConfigStore(#[from] AgentControlConfigError),

    /// The garbage collector ran with an empty list of current agents.
    #[error("garbage collector executed with empty current agents list")]
    MissingActiveAgents(),

    /// Fetched resources were missing the required labels.
    #[error("garbage collector fetched resources without required labels")]
    MissingLabels(),

    /// Fetched resources were missing the required annotations.
    #[error("garbage collector fetched resources without required annotations")]
    MissingAnnotations(),

    /// An agent type id could not be parsed.
    #[error("unable to parse AgentTypeID: {0}")]
    ParsingAgentType(#[from] AgentTypeIDError),

    /// An agent id could not be parsed.
    #[error("unable to parse AgentID: {0}")]
    ParsingAgentId(#[from] AgentIDError),
}
