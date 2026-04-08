use std::convert::Infallible;

use kube::api::{DynamicObject, ObjectMeta};
use nr_auth::key::{
    creator::{Creator, KeyPair, KeyType, PublicKeyPem},
    rsa::rsa,
};
use serde_json::json;
use tracing::info;

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::{
    agent_control::{agent_id::AgentID, config::secret_type_meta},
    cli::{
        common::{
            error::CliError,
            proxy_config::ProxyConfig,
            region::{Region, region_parser},
            system_identity::{
                ProvisioningMethod, SystemIdentityArgs, SystemIdentityData, SystemIdentitySpec,
                provide_identity,
            },
        },
        k8s::{errors::K8sCliError, utils::try_new_k8s_client},
    },
    k8s::{client::K8sObjectKey, labels::Labels},
};

const CLIENT_ID_SECRET_KEY: &str = "CLIENT_ID";
const PRIVATE_KEY_SECRET_KEY: &str = "private_key";

/// Registers a System Identity for Agent Control configuration
#[derive(Debug, clap::Parser)]
pub struct Args {
    /// Identity Secret name
    #[arg(long, required = true)]
    secret_name: String,

    /// New Relic region
    #[arg(long, value_parser = region_parser(), required = true)]
    region: Region,

    /// Identity configuration
    #[command(flatten)]
    identity: SystemIdentityArgs,

    /// Proxy configuration
    #[command(flatten)]
    proxy_config: Option<ProxyConfig>,
}

/// Data required to register a System Identity, information from [Args] after validation.
#[derive(Debug)]
pub struct IdentityRegistrationSpec {
    secret_name: String,
    region: Region,
    identity: SystemIdentitySpec,
    proxy_config: Option<ProxyConfig>,
}

impl Args {
    /// Performs additional args validation (not covered by clap's arguments)
    pub fn validate(self) -> Result<IdentityRegistrationSpec, String> {
        if let Some(proxy_config) = self.proxy_config.clone()
            && let Err(err) = crate::http::config::ProxyConfig::try_from(proxy_config)
        {
            return Err(format!("invalid proxy configuration: {err}"));
        }
        Ok(IdentityRegistrationSpec {
            secret_name: self.secret_name,
            region: self.region,
            identity: self.identity.validate()?,
            proxy_config: self.proxy_config,
        })
    }
}

/// Registers a System Identity as defined in the provided spec.
pub fn register_system_identity(
    namespace: &str,
    spec: IdentityRegistrationSpec,
) -> Result<(), K8sCliError> {
    let k8s_client = try_new_k8s_client()?;
    provide_system_identity_secret(namespace, spec, &k8s_client, provide_identity)
}

/// Helper function to implement [register_system_identity] while allowing the usage of mocks for the k8s client
/// and `provide_identity_fn`.
/// The function `provide_identity_fn` is expected to return the client_id of the registered System Identity.
/// The generated private key is stored in a Kubernetes Secret alongside the client_id.
fn provide_system_identity_secret<F>(
    namespace: &str,
    spec: IdentityRegistrationSpec,
    k8s_client: &SyncK8sClient,
    provide_identity_fn: F,
) -> Result<(), K8sCliError>
where
    F: Fn(
        &ProvisioningMethod,
        Region,
        Option<ProxyConfig>,
        PublicKeyHolder,
    ) -> Result<String, CliError>,
{
    let secret_object_key = K8sObjectKey {
        name: &spec.secret_name,
        namespace,
    };

    info!(
        secret_name = spec.secret_name, %namespace,
        "Checking if the System Identity secret already exists..."
    );
    if secret_already_exists(secret_object_key, k8s_client)? {
        info!("System Identity already exists, all setup.");
        return Ok(());
    }
    info!("Secret is not present, creating system identity");

    let KeyPair {
        private_key,
        public_key,
    } = rsa(&KeyType::Rsa4096).map_err(|err| {
        K8sCliError::Generic(format!(
            "failure building key-pair for System Identity: {err}"
        ))
    })?;
    let pk_holder = PublicKeyHolder { public_key };
    let SystemIdentityData::Provision(provisioning_method) = &spec.identity.system_identity_data
    else {
        return Err(K8sCliError::Generic(
            "existing identity is not supported in the k8s cli".to_string(),
        ));
    };

    let client_id = provide_identity_fn(
        provisioning_method,
        spec.region,
        spec.proxy_config,
        pk_holder,
    )
    .map_err(|err| {
        K8sCliError::Generic(format!("failure registering the System Identity: {err}"))
    })?;

    let private_key = String::from_utf8(private_key).map_err(|err| {
        K8sCliError::Generic(format!(
            "failure decoding System Identity private-key: {err}"
        ))
    })?;

    let secret_content = json!({"stringData": {
        CLIENT_ID_SECRET_KEY: client_id,
        PRIVATE_KEY_SECRET_KEY: private_key
    }});

    let secret = secret_dynamic_object(secret_object_key, secret_content);

    k8s_client.apply_dynamic_object(&secret).map_err(|err| {
        K8sCliError::ApplyResource(format!(
            "failure creating the System Identity secret: {err}"
        ))
    })?;

    info!(
        secret_name = spec.secret_name, %namespace,
        "System identity successfully stored"
    );

    Ok(())
}

fn secret_already_exists(
    object_key: K8sObjectKey<'_>,
    k8s_client: &SyncK8sClient,
) -> Result<bool, K8sCliError> {
    k8s_client
        .get_dynamic_object(&secret_type_meta(), object_key)
        .map(|s| s.is_some())
        .map_err(|err| {
            K8sCliError::GetResource(format!(
                "failure checking if the system identity secret exists: {err}"
            ))
        })
}

/// Builds the secret to store the System Identity data with the provided data. It includes the [Labels] corresponding
/// to the agent [AgentID::AgentControl].
fn secret_dynamic_object(object_key: K8sObjectKey<'_>, data: serde_json::Value) -> DynamicObject {
    let labels = Labels::new(&AgentID::AgentControl);
    DynamicObject {
        types: Some(secret_type_meta()),
        metadata: ObjectMeta {
            name: Some(object_key.name.to_string()),
            namespace: Some(object_key.namespace.to_string()),
            labels: Some(labels.get()),
            ..Default::default()
        },
        data,
    }
}

/// Helper struct to hold a public key an use it as [Creator] for System Identity provisioning.
struct PublicKeyHolder {
    public_key: PublicKeyPem,
}

impl Creator for PublicKeyHolder {
    type Error = Infallible;

    fn create(&self) -> Result<PublicKeyPem, Self::Error> {
        Ok(self.public_key.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::common::system_identity::ProvisioningMethod;
    use crate::k8s::client::MockSyncK8sClient;
    use assert_matches::assert_matches;
    use clap::{CommandFactory, FromArgMatches};
    use rstest::rstest;
    use std::{env::current_dir, path::PathBuf, sync::Arc};

    impl Default for IdentityRegistrationSpec {
        fn default() -> Self {
            IdentityRegistrationSpec {
                secret_name: "test-secret".to_string(),
                region: Region::US,
                identity: SystemIdentitySpec {
                    system_identity_data: SystemIdentityData::Provision(
                        ProvisioningMethod::ParentSecret {
                            secret: "secret".to_string(),
                            parent_client_id: "parent_client_id".to_string(),
                            organization_id: "org_id".to_string(),
                        },
                    ),
                    private_key_path: PathBuf::from("/test/key"),
                },
                proxy_config: None,
            }
        }
    }

    #[rstest]
    #[case::token_based_identity(
        || String::from("--secret-name s  --region us --auth-private-key-path /some/path --auth-parent-token TOKEN --auth-parent-client-id id --organization-id org-id")
    )]
    #[case::client_secret_based_identity(
        || String::from("--secret-name s --region us --auth-private-key-path /some/path --auth-parent-client-secret SECRET --auth-parent-client-id id --organization-id org-id")
    )]
    fn test_args_validation(#[case] args: fn() -> String) {
        let cmd = Args::command().no_binary_name(true);
        let matches = cmd
            .try_get_matches_from(args().split_ascii_whitespace())
            .expect("arguments should be valid");
        let args = Args::from_arg_matches(&matches).expect("should create the struct back");
        assert!(args.validate().is_ok());
    }

    #[rstest]
    #[case::missing_private_key_path(
        || String::from("--secret-name s --region us --auth-client-id some-id")
    )]
    #[case::no_identity_method(
        || format!("--secret-name s --region us --auth-private-key-path {}", current_dir().unwrap().display())
    )]
    #[case::missing_org_id_with_token(
        || String::from("--secret-name s --region us --auth-private-key-path /p --auth-parent-token TOKEN --auth-parent-client-id id")
    )]
    #[case::missing_parent_client_id_with_secret(
        || String::from("--secret-name s --region us --auth-private-key-path /p --auth-parent-client-secret SECRET --organization-id org-id")
    )]
    #[case::missing_org_id_with_secret(
        || String::from("--secret-name s --region us --auth-private-key-path /p --auth-parent-client-secret SECRET --auth-parent-client-id id")
    )]
    #[case::invalid_proxy_config(
        || String::from("--secret-name s --region us --auth-private-key-path /p --auth-parent-client-secret SECRET --auth-parent-client-id id --organization-id org-id --proxy-url https::/invalid")
    )]
    fn test_args_validation_errors(#[case] args: fn() -> String) {
        let cmd = Args::command().no_binary_name(true);
        let matches = cmd
            .try_get_matches_from(args().split_ascii_whitespace())
            .expect("arguments should be valid");
        let args = Args::from_arg_matches(&matches).expect("should create the struct back");
        assert_matches!(args.validate(), Err(_));
    }

    fn empty_dynamic_object() -> Arc<DynamicObject> {
        Arc::new(DynamicObject {
            types: None,
            metadata: Default::default(),
            data: serde_json::Value::Null,
        })
    }

    fn mock_secret_not_found() -> MockSyncK8sClient {
        let mut mock_client = MockSyncK8sClient::new();
        mock_client
            .expect_get_dynamic_object()
            .once()
            .returning(|_, _| Ok(None));
        mock_client
    }

    #[test]
    fn test_secret_already_exists_skips_creation() {
        let mut mock_client = MockSyncK8sClient::new();

        mock_client
            .expect_get_dynamic_object()
            .once()
            .returning(|_, _| Ok(Some(empty_dynamic_object())));

        mock_client.expect_apply_dynamic_object().never();

        provide_system_identity_secret(
            "test-namespace",
            IdentityRegistrationSpec::default(),
            &mock_client,
            |_, _, _, _| panic!("identity provider should not be called"),
        )
        .expect("system identity should be provided successfully");
    }

    #[test]
    fn test_creates_secret_when_not_present() {
        let mut mock_client = MockSyncK8sClient::new();

        mock_client
            .expect_get_dynamic_object()
            .once()
            .returning(|_, _| Ok(None));

        mock_client
            .expect_apply_dynamic_object()
            .once()
            .withf(|obj| {
                obj.metadata.name.as_deref() == Some("test-secret")
                    && obj.metadata.namespace.as_deref() == Some("test-namespace")
                    && obj.data["stringData"][CLIENT_ID_SECRET_KEY].as_str()
                        == Some("new-client-id")
                    && obj.data["stringData"][PRIVATE_KEY_SECRET_KEY]
                        .as_str()
                        .is_some_and(|k| k.contains("-----BEGIN PRIVATE KEY-----"))
            })
            .returning(|_| Ok(()));

        provide_system_identity_secret(
            "test-namespace",
            IdentityRegistrationSpec::default(),
            &mock_client,
            move |_, _, _, _| Ok("new-client-id".to_string()),
        )
        .expect("system identity should be provided successfully");
    }

    #[test]
    fn test_get_dynamic_object_error_returns_error() {
        let mut mock_client = MockSyncK8sClient::new();
        mock_client
            .expect_get_dynamic_object()
            .once()
            .returning(|_, _| {
                Err(crate::k8s::error::K8sError::Generic(
                    "k8s error".to_string(),
                ))
            });

        let result = provide_system_identity_secret(
            "test-namespace",
            IdentityRegistrationSpec::default(),
            &mock_client,
            |_, _, _, _| panic!("should not be called"),
        );
        assert_matches!(result, Err(K8sCliError::GetResource(_)));
    }

    #[test]
    fn test_provide_identity_error_returns_error() {
        let mock_client = mock_secret_not_found();

        let result = provide_system_identity_secret(
            "test-namespace",
            IdentityRegistrationSpec::default(),
            &mock_client,
            |_, _, _, _| Err(CliError::Command("identity failure".to_string())),
        );
        assert_matches!(result, Err(K8sCliError::Generic(_)));
    }

    #[test]
    fn test_apply_dynamic_object_error_returns_error() {
        let mut mock_client = mock_secret_not_found();
        mock_client
            .expect_apply_dynamic_object()
            .once()
            .returning(|_| {
                Err(crate::k8s::error::K8sError::Generic(
                    "apply failed".to_string(),
                ))
            });

        let result = provide_system_identity_secret(
            "test-namespace",
            IdentityRegistrationSpec::default(),
            &mock_client,
            move |_, _, _, _| Ok("id".to_string()),
        );
        assert_matches!(result, Err(K8sCliError::ApplyResource(_)));
    }
}
