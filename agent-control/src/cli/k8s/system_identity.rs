use k8s_openapi::{Resource as _, api::core::v1::Secret};
use kube::api::TypeMeta;
use tracing::info;

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::{
    cli::{
        common::{
            proxy_config::ProxyConfig,
            region::{Region, region_parser},
            system_identity::{SystemIdentityArgs, SytemIdentitySpec},
        },
        k8s::{errors::K8sCliError, utils::try_new_k8s_client},
    },
    k8s::client::K8sObjectKey,
};

const CLIENT_ID_SECRET_KEY: &str = "CLIENT_ID";
const PRIVATE_KEY_SECRET_KEY: &str = "private_key";

/// Registers the System Identity
#[derive(Debug, clap::Parser)]
pub struct Args {
    /// Identity Secret name
    #[arg(long, required = true)]
    secret_name: String,

    /// Identity Secret namespace
    #[arg(long, required = true)]
    secret_namespace: String,

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
    secret_namespace: String,
    region: Region,
    identity: SytemIdentitySpec,
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
            secret_namespace: self.secret_namespace,
            region: self.region,
            identity: self.identity.validate()?,
            proxy_config: self.proxy_config,
        })
    }
}

pub fn register_system_identity(spec: IdentityRegistrationSpec) -> Result<(), K8sCliError> {
    let k8s_client = try_new_k8s_client()?;
    let secret_object_key = K8sObjectKey {
        name: &spec.secret_name,
        namespace: &spec.secret_namespace,
    };

    info!(
        secret_name = spec.secret_name,
        secret_namespace = spec.secret_namespace,
        "Checking if the System Identity secret already exists..."
    );
    if secret_already_exits(secret_object_key, k8s_client)? {
        info!("System Identity already exists, all setup.");
        return Ok(());
    }

    info!("Secret is not present, creating system identity");

    todo!("...")
}

fn secret_already_exits(
    object_key: K8sObjectKey<'_>,
    k8s_client: SyncK8sClient,
) -> Result<bool, K8sCliError> {
    k8s_client
        .get_dynamic_object(&secret_type_meta(), object_key)
        .map(|s| s.is_some())
        .map_err(|err| {
            K8sCliError::GetResource(format!(
                "failed to check if the system identity secret exists: {err}"
            ))
        })
}

fn secret_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: Secret::API_VERSION.to_string(),
        kind: Secret::KIND.to_string(),
    }
}
