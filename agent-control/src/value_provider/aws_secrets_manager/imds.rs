//! Retrieval of EC2 instance profile credentials from the instance metadata service (IMDSv2).

use super::AwsSecretsManagerError;
use crate::http::client::{HttpClient, HttpResponseError};
use crate::http::config::{HttpConfig, ProxyConfig};
use aws_credential_types::Credentials;
use chrono::DateTime;
use http::Request;
use serde::Deserialize;
use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, SystemTime},
};

/// EC2 instance metadata service base URL.
pub const DEFAULT_IMDS_BASE_URL: &str = "http://169.254.169.254";
const IMDS_TOKEN_PATH: &str = "/latest/api/token";
const IMDS_ROLE_PATH: &str = "/latest/meta-data/iam/security-credentials/";
const IMDS_TOKEN_TTL_HEADER: &str = "x-aws-ec2-metadata-token-ttl-seconds";
const IMDS_TOKEN_HEADER: &str = "x-aws-ec2-metadata-token";
const IMDS_TOKEN_TTL_SECONDS: &str = "21600";
/// Instance profile credentials are refreshed when they expire within this margin.
const CREDENTIALS_REFRESH_MARGIN: Duration = Duration::from_secs(60);

/// Represents the AWS Instance Profile to obtain credentials from
pub struct AwsInstanceProfile {
    /// Dedicated client without proxy: IMDS is link-local and unreachable through a proxy.
    client: HttpClient,
    imds_base_url: String,
    /// Last credentials obtained from IMDS, reused until they are about to expire.
    cache: Mutex<Option<Credentials>>,
}

impl AwsInstanceProfile {
    pub fn try_new(timeout: Duration) -> Result<Self, AwsSecretsManagerError> {
        Self::with_base_url(timeout, DEFAULT_IMDS_BASE_URL.to_string())
    }

    pub fn with_base_url(
        timeout: Duration,
        imds_base_url: String,
    ) -> Result<Self, AwsSecretsManagerError> {
        let client = HttpClient::new(HttpConfig::new(timeout, timeout, ProxyConfig::default()))?;
        Ok(Self {
            client,
            imds_base_url,
            cache: Mutex::new(None),
        })
    }

    /// Returns cached credentials, fetching new ones from IMDS when they are about to expire.
    ///
    /// The lock is held during the fetch in order to avoid that concurrent callers refresh twice.
    pub fn credentials(&self) -> Result<Credentials, AwsSecretsManagerError> {
        let mut cached = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(credentials) = cached.as_ref().filter(|c| !is_expiring(c)) {
            return Ok(credentials.clone());
        }
        let fresh = fetch_instance_credentials(&self.client, &self.imds_base_url)?;
        *cached = Some(fresh.clone());
        Ok(fresh)
    }
}

/// IMDS security credentials response (only the fields we need).
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ImdsCredentialsResponse {
    access_key_id: String,
    secret_access_key: String,
    token: String,
    expiration: String,
}

/// Retrieves the instance profile credentials following the IMDSv2 flow.
pub fn fetch_instance_credentials(
    client: &HttpClient,
    imds_base_url: &str,
) -> Result<Credentials, AwsSecretsManagerError> {
    let token = imds_send(
        client,
        "PUT",
        format!("{imds_base_url}{IMDS_TOKEN_PATH}"),
        (IMDS_TOKEN_TTL_HEADER, IMDS_TOKEN_TTL_SECONDS),
    )
    .map_err(|e| match e {
        AwsSecretsManagerError::CredentialsRequest(reason) => {
            AwsSecretsManagerError::CredentialsRequest(format!(
                "could not get a token from the instance metadata service, check that the host is an EC2 instance with an IAM role attached: {reason}"
            ))
        }
        other => other,
    })?;

    let roles = imds_send(
        client,
        "GET",
        format!("{imds_base_url}{IMDS_ROLE_PATH}"),
        (IMDS_TOKEN_HEADER, &token),
    )?;
    let role = roles
        .lines()
        .map(str::trim)
        .find(|r| !r.is_empty())
        .ok_or_else(|| {
            AwsSecretsManagerError::CredentialsRequest(
                "no IAM role is attached to the instance".to_string(),
            )
        })?;

    let body = imds_send(
        client,
        "GET",
        format!("{imds_base_url}{IMDS_ROLE_PATH}{role}"),
        (IMDS_TOKEN_HEADER, &token),
    )?;
    let response: ImdsCredentialsResponse = serde_json::from_str(&body)
        .map_err(|e| AwsSecretsManagerError::DeserializeError(e.to_string()))?;

    let expiry = DateTime::parse_from_rfc3339(&response.expiration)
        .map_err(|e| AwsSecretsManagerError::DeserializeError(format!("expiration: {e}")))?;

    Ok(Credentials::new(
        response.access_key_id,
        response.secret_access_key,
        Some(response.token),
        Some(SystemTime::from(expiry)),
        "agent-control-instance-profile",
    ))
}

fn imds_send(
    client: &HttpClient,
    method: &str,
    url: String,
    header: (&str, &str),
) -> Result<String, AwsSecretsManagerError> {
    let request = Request::builder()
        .method(method)
        .uri(url)
        .header(header.0, header.1)
        .body(Vec::new())
        .map_err(|e| AwsSecretsManagerError::CredentialsRequest(e.to_string()))?;

    let response = client.send(request).map_err(|e| match e {
        HttpResponseError::UnsuccessfulResponse { status_code, body } => {
            AwsSecretsManagerError::CredentialsRequest(format!(
                "IMDS responded with status {status_code}: {}",
                String::from_utf8_lossy(&body)
            ))
        }
        _ => AwsSecretsManagerError::CredentialsRequest(e.to_string()),
    })?;

    String::from_utf8(response.into_body())
        .map_err(|e| AwsSecretsManagerError::DeserializeError(format!("invalid utf8: {e}")))
}

/// Whether the credentials are expired or about to expire. Credentials without expiry are
/// considered expiring so they are always refreshed.
fn is_expiring(credentials: &Credentials) -> bool {
    credentials
        .expiry()
        .is_none_or(|expiry| expiry <= SystemTime::now() + CREDENTIALS_REFRESH_MARGIN)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::http::config::{HttpConfig, ProxyConfig};
    use assert_matches::assert_matches;
    use httpmock::Method::{GET, PUT};
    use httpmock::{Mock, MockServer};
    use std::time::Duration;

    pub const ACCESS_KEY: &str = "AKIDTEST";
    pub const SECRET_KEY: &str = "test-secret-key";
    const SESSION_TOKEN: &str = "session-token";
    const IMDS_TOKEN: &str = "imds-token";
    const ROLE: &str = "test-role";
    const EXPIRATION: &str = "2999-01-01T00:00:00Z";

    fn client() -> HttpClient {
        let timeout = Duration::from_secs(5);
        HttpClient::new(HttpConfig::new(timeout, timeout, ProxyConfig::default())).unwrap()
    }

    /// Mocks the IMDSv2 flow and returns the token mock to count credential fetches.
    pub fn mock_imds<'a>(server: &'a MockServer, expiration: &str) -> Mock<'a> {
        let token_mock = server.mock(|when, then| {
            when.method(PUT)
                .path(IMDS_TOKEN_PATH)
                .header(IMDS_TOKEN_TTL_HEADER, IMDS_TOKEN_TTL_SECONDS);
            then.status(200).body(IMDS_TOKEN);
        });
        server.mock(|when, then| {
            when.method(GET)
                .path(IMDS_ROLE_PATH)
                .header(IMDS_TOKEN_HEADER, IMDS_TOKEN);
            then.status(200).body(format!("{ROLE}\n"));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path(format!("{IMDS_ROLE_PATH}{ROLE}"))
                .header(IMDS_TOKEN_HEADER, IMDS_TOKEN);
            then.status(200).body(format!(
                r#"{{"Code":"Success","AccessKeyId":"{ACCESS_KEY}","SecretAccessKey":"{SECRET_KEY}","Token":"{SESSION_TOKEN}","Expiration":"{expiration}"}}"#
            ));
        });
        token_mock
    }

    #[test]
    fn test_fetch_instance_credentials_success() {
        let server = MockServer::start();
        let token_mock = mock_imds(&server, EXPIRATION);

        let credentials = fetch_instance_credentials(&client(), &server.base_url()).unwrap();

        assert_eq!(credentials.access_key_id(), ACCESS_KEY);
        assert_eq!(credentials.secret_access_key(), SECRET_KEY);
        assert_eq!(credentials.session_token(), Some(SESSION_TOKEN));
        assert_eq!(
            credentials.expiry(),
            Some(SystemTime::from(
                DateTime::parse_from_rfc3339(EXPIRATION).unwrap()
            ))
        );
        token_mock.assert_calls(1);
    }

    #[test]
    fn test_fetch_instance_credentials_imds_unavailable() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(PUT).path(IMDS_TOKEN_PATH);
            then.status(500).body("internal error");
        });

        assert_matches!(
            fetch_instance_credentials(&client(), &server.base_url()),
            Err(AwsSecretsManagerError::CredentialsRequest(_))
        );
    }

    #[test]
    fn test_fetch_instance_credentials_no_role_attached() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(PUT).path(IMDS_TOKEN_PATH);
            then.status(200).body(IMDS_TOKEN);
        });
        server.mock(|when, then| {
            when.method(GET).path(IMDS_ROLE_PATH);
            then.status(200).body("");
        });

        assert_matches!(
            fetch_instance_credentials(&client(), &server.base_url()),
            Err(AwsSecretsManagerError::CredentialsRequest(_))
        );
    }

    #[test]
    fn test_fetch_instance_credentials_invalid_json() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(PUT).path(IMDS_TOKEN_PATH);
            then.status(200).body(IMDS_TOKEN);
        });
        server.mock(|when, then| {
            when.method(GET).path(IMDS_ROLE_PATH);
            then.status(200).body(ROLE);
        });
        server.mock(|when, then| {
            when.method(GET).path(format!("{IMDS_ROLE_PATH}{ROLE}"));
            then.status(200).body("not json");
        });

        assert_matches!(
            fetch_instance_credentials(&client(), &server.base_url()),
            Err(AwsSecretsManagerError::DeserializeError(_))
        );
    }

    #[test]
    fn test_fetch_instance_credentials_invalid_expiration() {
        let server = MockServer::start();
        mock_imds(&server, "not-a-date");

        assert_matches!(
            fetch_instance_credentials(&client(), &server.base_url()),
            Err(AwsSecretsManagerError::DeserializeError(_))
        );
    }
}
