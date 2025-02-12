use rustls::RootCertStore;
use rustls_native_certs::load_native_certs;
use tracing::warn;

#[derive(thiserror::Error, Debug)]
pub enum TLSConfigBuildingError {
    #[error("error building tls config: `{0}`")]
    BuildingError(String),
}

/// Install the default rustls crypto provider, this needs to be executed early in the process, check
/// <https://docs.rs/rustls/latest/rustls/crypto/struct.CryptoProvider.html#method.install_default> for details.
pub fn install_rustls_default_crypto_provider() {
    rustls::crypto::ring::default_provider().install_default().unwrap_or_else(|_| {
        warn!("rustls default crypto provider was already installed for this process, this has no effect")
    })
}

pub fn root_store_with_native_certs() -> Result<RootCertStore, TLSConfigBuildingError> {
    let mut root_store = rustls::RootCertStore::empty();
    load_native_certs().certs.iter().try_for_each(|cert| {
        root_store.add(cert.to_owned()).map_err(|e| {
            TLSConfigBuildingError::BuildingError(format!("cannot add system certificates: {}", e))
        })
    })?;
    Ok(root_store)
}
