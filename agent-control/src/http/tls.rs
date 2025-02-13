use tracing::warn;

/// Install the default rustls crypto provider, this needs to be executed early in the process, check
/// <https://docs.rs/rustls/latest/rustls/crypto/struct.CryptoProvider.html#method.install_default> for details.
pub fn install_rustls_default_crypto_provider() {
    rustls::crypto::ring::default_provider().install_default().unwrap_or_else(|_| {
        warn!("rustls default crypto provider was already installed for this process, this has no effect")
    })
}
