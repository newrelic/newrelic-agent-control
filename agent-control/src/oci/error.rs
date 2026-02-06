//! Error definition and tooling for the oci module.

use oci_client::errors::{OciDistributionError, OciErrorCode};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OciClientError {
    #[error("could not build the OCI client: {0}")]
    Build(String),
    #[error("failure pulling image manifest: {0}")]
    PullManifest(OciErrorMessage),
    #[error("failure pulling blob: {0}")]
    PullBlob(OciErrorMessage),
}

/// Simple string wrapper to represent curated messages coming from [oci_client].
#[derive(Debug, Error)]
#[error("{0}")]
pub struct OciErrorMessage(String);

impl From<String> for OciErrorMessage {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<OciDistributionError> for OciErrorMessage {
    fn from(err: OciDistributionError) -> Self {
        match err {
            OciDistributionError::RegistryError { ref envelope, .. } => {
                let Some(oci_err) = envelope.errors.first() else {
                    return format!("registry error: {err}").into();
                };

                let err_msg = match oci_err.code {
                    OciErrorCode::ManifestUnknown | OciErrorCode::NotFound => {
                        format!("the requested version does not exist in the registry: {err}")
                    }
                    OciErrorCode::NameUnknown | OciErrorCode::NameInvalid => {
                        format!("the repository name is invalid or not found: {err}")
                    }
                    OciErrorCode::Unauthorized | OciErrorCode::Denied => {
                        format!("access denied, check credentials: {err}")
                    }
                    OciErrorCode::Toomanyrequests => {
                        format!("rate limit exceeded: the registry is throttling requests: {err}")
                    }
                    _ => format!("registry error ({:?}): {}", oci_err.code, oci_err.message),
                };
                format!("the registry and repository are not found or reachable: {err_msg}").into()
            }
            // Use _ to catch all other variants like AuthenticationFailure, etc.
            _ => format!("registry error: {err}").into(),
        }
    }
}

#[cfg(test)]
mod tests {

    use oci_client::errors::{OciEnvelope, OciError};
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::manifest_unknown(
        OciDistributionError::RegistryError {
        envelope: OciEnvelope { errors: vec![OciError { code: OciErrorCode::ManifestUnknown, message: "not found".into(), detail: Default::default() }] },
        url: "url".into()
            },
        "the requested version does not exist"
    )]
    #[case::access_denied(
        OciDistributionError::RegistryError {
        envelope: OciEnvelope { errors: vec![OciError { code: OciErrorCode::Denied, message: "forbidden".into(), detail: Default::default() }] },
        url: "url".into()
            },
        "access denied"
    )]
    #[case::empty_envelope(
        OciDistributionError::RegistryError {
        envelope: OciEnvelope { errors: vec![] },
        url: "url".into()
            },
        "Registry error:"
    )]
    fn test_from_oci_error_mapping(
        #[case] input_error: OciDistributionError,
        #[case] expected_msg: &str,
    ) {
        let err = OciErrorMessage::from(input_error);
        assert!(err.0.contains(expected_msg));
    }

    #[test]
    fn test_connection_error_fallback() {
        let oci_err = OciDistributionError::AuthenticationFailure("bad login".to_string());
        let err = OciErrorMessage::from(oci_err);
        assert!(err.0.contains("bad loging"));
    }
}
