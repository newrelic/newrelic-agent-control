use nix::NixPath;
use rustls::{ClientConfig, RootCertStore};
use rustls_native_certs::load_native_certs;
use std::path::Path;
use tracing::warn;

const CERT_EXTENSION: &str = "pem";

#[derive(thiserror::Error, Debug)]
pub enum TLSConfigBuildingError {
    #[error("error building tls config: `{0}`")]
    BuildingError(String),
    #[error("IO error: `{0}`")]
    IOError(String),
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

/// It builds rustls client configuration including the system native certificates and the certificates in the file
/// and dir provided as arguments if any of the paths is empty, it is ignored.
/// It return an error if there are issues reading the provided paths or if invalid certificates are found.
pub fn build_tls_config(
    pem_file: &Path,
    pem_files_dir: &Path,
) -> Result<ClientConfig, TLSConfigBuildingError> {
    let mut root_store = root_store_with_native_certs()?;

    // Add custom certificates from file
    if !pem_file.is_empty() {
        add_certs_from_file(&mut root_store, pem_file)?;
    }

    // Add custom certificates from dir
    if !pem_files_dir.is_empty() {
        let dir_entries = std::fs::read_dir(pem_files_dir).map_err(|e| {
            TLSConfigBuildingError::BuildingError(format!(
                "cannot read directory {}: {}",
                pem_files_dir.to_string_lossy(),
                e
            ))
        })?;
        // Handle errors reading the items directory
        let dir_entries = dir_entries.map(|entry| {
            entry.map_err(|err| {
                TLSConfigBuildingError::IOError(format!(
                    "error reading directory {}: {}",
                    pem_files_dir.to_string_lossy(),
                    err
                ))
            })
        });
        // Add custom certificates from each file with pem extension in the directory.
        for dir_entry_result in dir_entries {
            let file_path = dir_entry_result?.path();
            if path_has_cert_extension(&file_path) {
                add_certs_from_file(&mut root_store, &file_path)?;
            }
        }
    }

    Ok(rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth())
}

/// Checks if the provided path has certificate extension
fn path_has_cert_extension(path: &Path) -> bool {
    match path.extension() {
        Some(extension) => extension == CERT_EXTENSION,
        None => false,
    }
}

/// Add a custom certificate to the provided [RootCertStore]. Errors if the file cannot be read or the certificate is
/// invalid.
fn add_certs_from_file(
    root_store: &mut RootCertStore,
    pem_path: &Path,
) -> Result<(), TLSConfigBuildingError> {
    let mut pem = std::io::BufReader::new(std::fs::File::open(pem_path).map_err(|e| {
        TLSConfigBuildingError::BuildingError(format!(
            "cannot read custom certificate {}: {}",
            pem_path.to_string_lossy(),
            e
        ))
    })?);
    // Handle invalid certificates
    let certs_iter = rustls_pemfile::certs(&mut pem).map(|cert_result| {
        cert_result.map_err(|err| {
            TLSConfigBuildingError::BuildingError(format!(
                "invalid custom certificate {}: {}",
                pem_path.to_string_lossy(),
                err,
            ))
        })
    });
    // Add certificates from file
    for cert_result in certs_iter {
        let cert = cert_result?;
        root_store.add(cert).map_err(|e| {
            TLSConfigBuildingError::BuildingError(format!(
                "cannot add custom certificate {}: {}",
                pem_path.to_string_lossy(),
                e
            ))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::tempdir;

    const VALID_TESTING_CERT: &str = r#"-----BEGIN CERTIFICATE-----
MIICljCCAX4CCQDd10xL2UoK6jANBgkqhkiG9w0BAQsFADANMQswCQYDVQQGEwJl
czAeFw0yNDEwMjMxMzUzNTBaFw0yOTEwMjIxMzUzNTBaMA0xCzAJBgNVBAYTAmVz
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAsg0LUPDa1EmD17CcKORF
dDTTfJNpPSZk4+Z1NHfL3pa1OwYEvtwqIetiu0u2Y7iPObnLb5AibqVp2gRBJ9kl
E85lm8oDIZ4xahzlBacKkUVXPTI/l+vugcV/x5wL+bONXXJGboMQpdbjENG6fBkM
5SmEubs1Bto6nPkd+UvUoP8F8RPMpkgq49MmY0KeySfn6Qu2pFqboyAFBr70Of60
LqONmkEr6GvR9EP+XT55+b40i73uUmdtFQh9Xdy+JiAiYer41yKM2nMPYLu8CkN8
CYWlws4qVzcbvb2Yc0AUgSDoh5uBT5VFyPO/kVR0hFQUVWBJiMFblhGXWq4QQfxU
kwIDAQABMA0GCSqGSIb3DQEBCwUAA4IBAQBefiSBicwVlWeMwl6xTRHEX43VnM12
KJln0Vwlp/4M72OIEoRVBLUax77uuJPJYEA333/dHrsr9N4B9/QRfYyCmvTXspLo
jgVmP+LsKoujyrONr5zmxCvH23Lu3CY4AD1Wn9B59MEZYyhO29F/2ZFz9/CrmYsR
GoGcH6dzLXvsFnjTWxET45kEgebIawDpETht/joFiLe5dSfL6qZMxozfLj67I5g9
ZTCQMUeixFNXn8hgPk2GXa0E3Qf0HV6R++SFDCNtRKK2kaidq66PdKphNP1fXK9S
PmmEIikVmq+diZVAViKF7+4aXMFHYuCsx+MgazO6d2StrFHrw19TTDPr
-----END CERTIFICATE-----"#;

    const INVALID_TESTING_CERT: &str =
        "-----BEGIN CERTIFICATE-----\ninvalid!\n-----END CERTIFICATE-----";

    #[test]
    fn test_build_tls_config_with_no_certificates() {
        install_rustls_default_crypto_provider();

        let pem_file = PathBuf::default();
        let pem_dir = PathBuf::default();
        let config = build_tls_config(pem_file.as_path(), pem_dir.as_path());
        assert!(config.is_ok(), "Expected Ok config got {:?}", config);
    }

    #[test]
    fn test_build_tls_config_with_not_existing_certificate_file() {
        install_rustls_default_crypto_provider();
        let pem_file = Path::new("non-existing.pem");
        let pem_dir = PathBuf::default();
        let config = build_tls_config(pem_file, pem_dir.as_path());
        assert_matches!(
            config.unwrap_err(),
            TLSConfigBuildingError::BuildingError(s) => {
                assert!(s.contains("non-existing.pem"))
            }
        );
    }

    #[test]
    fn test_build_tls_config_with_invalid_certificate_file() {
        install_rustls_default_crypto_provider();

        let dir = tempdir().unwrap();
        let file_path = dir.path().join("invalid_cert.pem");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "{INVALID_TESTING_CERT}").unwrap();

        let pem_dir = PathBuf::default();
        let config = build_tls_config(file_path.as_path(), pem_dir.as_path());
        assert_matches!(
            config.unwrap_err(),
            TLSConfigBuildingError::BuildingError(s) => {
                assert!(s.contains("invalid_cert.pem"))
            }
        );
    }

    #[test]
    fn test_build_tls_config_with_valid_certificate_file() {
        install_rustls_default_crypto_provider();

        let dir = tempdir().unwrap();
        let file_path = dir.path().join("valid_cert.pem");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "{VALID_TESTING_CERT}").unwrap();

        let pem_dir = PathBuf::default();
        let config = build_tls_config(file_path.as_path(), pem_dir.as_path());
        assert!(config.is_ok(), "{:?}", config);
    }

    #[test]
    fn test_build_tls_config_with_invalid_directory() {
        install_rustls_default_crypto_provider();

        let pem_file = PathBuf::default();
        let pem_dir = Path::new("non-existing-dir");
        let config = build_tls_config(pem_file.as_path(), pem_dir);
        assert_matches!(
            config.unwrap_err(),
            TLSConfigBuildingError::BuildingError(s) => {
                assert!(s.contains("non-existing-dir"))
            }
        );
    }

    #[test]
    fn test_build_tls_config_with_valid_directory() {
        install_rustls_default_crypto_provider();
        // Cert file
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("valid_cert.pem");
        let mut file = File::create(file_path).unwrap();
        writeln!(file, "{VALID_TESTING_CERT}").unwrap();
        // Unrelated file
        let file_path = dir.path().join("no-cert-file");
        let mut file = File::create(file_path).unwrap();
        writeln!(file, "some content").unwrap();
        // Invalid cert in no '.pem' file
        let file_path = dir.path().join("invalid-cert.txt");
        let mut file = File::create(file_path).unwrap();
        writeln!(file, "{INVALID_TESTING_CERT}").unwrap();

        let file_path = PathBuf::default();
        let config = build_tls_config(file_path.as_path(), dir.path());
        assert!(config.is_ok());
    }

    #[test]
    fn test_add_certs_from_file_with_invalid_file() {
        install_rustls_default_crypto_provider();

        let dir = tempdir().unwrap();
        let file_path = dir.path().join("invalid_cert.pem");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "{INVALID_TESTING_CERT}").unwrap();

        let mut root_store = RootCertStore::empty();
        let result = add_certs_from_file(&mut root_store, &file_path);
        assert_matches!(
            result.unwrap_err(),
            TLSConfigBuildingError::BuildingError(s) => {
                assert!(s.contains("invalid_cert.pem"))
            }
        );
        assert!(root_store.is_empty())
    }

    #[test]
    fn test_add_certs_from_file_with_valid_file() {
        install_rustls_default_crypto_provider();

        let dir = tempdir().unwrap();
        let file_path = dir.path().join("valid_cert.pem");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "{VALID_TESTING_CERT}").unwrap();

        let mut root_store = RootCertStore::empty();
        let result = add_certs_from_file(&mut root_store, &file_path);
        assert!(result.is_ok());
        assert_eq!(
            root_store.len(),
            1,
            "The custom certificate should be added"
        );
    }
}
