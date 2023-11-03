use kube::config::KubeconfigError;

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
}
