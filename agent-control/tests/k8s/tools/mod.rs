/// Contains helpers to execute the agent-control binary (compiled with the k8s feature)
/// and specific initial configuration. Any helper receiving a `folder_name` assumes that the folder exists
/// in the path `tests/k8s/data/`.
pub mod agent_control;
pub mod instance_id;
/// Provides tools to perform queries through the k8s API in order to perform assertions.
pub mod k8s_api;
/// Provides a k8s testing environment.
pub mod k8s_env;
/// Defines the Foo CRD to be created and used in testing k8s clusters.
pub mod test_crd;
