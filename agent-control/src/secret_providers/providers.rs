use serde::Deserialize;
use thiserror::Error;
use crate::secret_providers::vault::{Vault, VaultBuildError, VaultConfig};

#[derive(Error, Debug)]
pub enum SecretProvidersError {
    #[error("could not build the vault client: {0}")]
    VaultBuildError(#[from] VaultBuildError),
}

#[derive(PartialEq, Deserialize, Clone, Debug, Default)]
pub struct SecretProvidorsConfig {
    #[serde(default)]
    pub(crate) vault: Option<VaultConfig>,
}

pub struct SecretProviders {
    pub vault: Option<Vault>,
}

pub fn try_init_providers(config: SecretProvidorsConfig) -> Result<SecretProviders, SecretProvidersError> {
    if let Some(vault_config) = config.vault {
        return Ok(SecretProviders {
            vault: Some(Vault::try_build(vault_config)?),
        });
    }

    Ok(SecretProviders { vault: None })
}