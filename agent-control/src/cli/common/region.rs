//! This module defines configuration related to region.
use http::Uri;
use nr_auth::{
    parameters::Environments, system_identity::input_data::environment::NewRelicEnvironment,
};

const OPAMP_ENDPOINT_US: &str = "https://opamp.service.newrelic.com/v1/opamp";
const OPAMP_ENDPOINT_EU: &str = "https://opamp.service.eu.newrelic.com/v1/opamp";
const OPAMP_ENDPOINT_JP: &str = "https://opamp.service.jp.newrelic.com/v1/opamp";

const OPAMP_ENDPOINT_STAGING: &str = "https://opamp.staging-service.newrelic.com/v1/opamp";

const PUBLIC_KEY_ENDPOINT_US: &str =
    "https://publickeys.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json";
const PUBLIC_KEY_ENDPOINT_EU: &str =
    "https://publickeys.eu.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json";
const PUBLIC_KEY_ENDPOINT_JP: &str =
    "https://publickeys.jp.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json";
const PUBLIC_KEY_ENDPOINT_STAGING: &str =
    "https://staging-publickeys.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json";

const OTLP_URL_STAGING: &str = "staging-otlp.nr-data.net";
const OTLP_URL_JP: &str = "otlp.jp.nr-data.net";
const OTLP_URL_EU: &str = "otlp.eu01.nr-data.net";
const OTLP_URL_US: &str = "otlp.nr-data.net";

/// Represents the supported region and defines related fields. It cannot wrap the [Environments] enum
/// due to clap limitations. Re-defining the enum is simpler than extending and using some mapping
/// tool such as [clap::builder::TypedValueParser::map].
#[derive(Debug, Copy, Clone, PartialEq, clap::ValueEnum)]
pub enum Region {
    /// United States region.
    US,
    /// European Union region.
    EU,
    /// Japan region.
    JP,
    /// Staging environment (aliased as `stg`).
    #[value(alias = "stg")]
    STAGING,
}

impl From<Region> for Environments {
    fn from(value: Region) -> Self {
        match value {
            Region::US => Environments::US,
            Region::EU => Environments::EU,
            Region::JP => Environments::JP,
            Region::STAGING => Environments::STAGING,
        }
    }
}

impl From<Region> for NewRelicEnvironment {
    fn from(value: Region) -> Self {
        Environments::from(value).into()
    }
}

impl Region {
    /// Returns the OpAMP endpoint for this region.
    pub fn opamp_endpoint(&self) -> Uri {
        match &self {
            Self::US => OPAMP_ENDPOINT_US,
            Self::EU => OPAMP_ENDPOINT_EU,
            Self::JP => OPAMP_ENDPOINT_JP,
            Self::STAGING => OPAMP_ENDPOINT_STAGING,
        }
        .parse()
        .expect("known uris should be valid")
    }

    /// Returns the public-key (JWKS) endpoint used for signature validation in this region.
    pub fn public_key_endpoint(&self) -> Uri {
        match &self {
            Self::US => PUBLIC_KEY_ENDPOINT_US,
            Self::EU => PUBLIC_KEY_ENDPOINT_EU,
            Self::JP => PUBLIC_KEY_ENDPOINT_JP,
            Self::STAGING => PUBLIC_KEY_ENDPOINT_STAGING,
        }
        .parse()
        .expect("known uris should be valid")
    }

    /// Returns the OTLP exporter endpoint for this region.
    pub fn otel_endpoint(&self) -> Uri {
        let host = match &self {
            Self::US => OTLP_URL_US,
            Self::EU => OTLP_URL_EU,
            Self::JP => OTLP_URL_JP,
            Self::STAGING => OTLP_URL_STAGING,
        };
        let endpoint = format!("https://{}:4317", host);
        endpoint.parse().expect("known uris should be valid")
    }

    /// Returns the token renewal endpoint for this region, also needed in Agent Control configuration.
    pub fn token_renewal_endpoint(&self) -> Uri {
        NewRelicEnvironment::from(self.to_owned()).token_renewal_endpoint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(Region::US, Environments::US)]
    #[case(Region::EU, Environments::EU)]
    #[case(Region::JP, Environments::JP)]
    #[case(Region::STAGING, Environments::STAGING)]
    fn test_environments_conversion(#[case] region: Region, #[case] expected: Environments) {
        assert_eq!(Environments::from(region), expected);
    }

    #[rstest]
    #[case(Region::US, "https://opamp.service.newrelic.com/v1/opamp")]
    #[case(Region::EU, "https://opamp.service.eu.newrelic.com/v1/opamp")]
    #[case(Region::JP, "https://opamp.service.jp.newrelic.com/v1/opamp")]
    #[case(Region::STAGING, "https://opamp.staging-service.newrelic.com/v1/opamp")]
    fn test_opamp_endpoint(#[case] region: Region, #[case] expected_endpoint: &str) {
        assert_eq!(
            region.opamp_endpoint().to_string(),
            expected_endpoint.to_string()
        );
    }

    #[rstest]
    #[case(
        Region::US,
        "https://publickeys.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json"
    )]
    #[case(
        Region::EU,
        "https://publickeys.eu.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json"
    )]
    #[case(
        Region::JP,
        "https://publickeys.jp.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json"
    )]
    #[case(
        Region::STAGING,
        "https://staging-publickeys.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json"
    )]
    fn test_public_key_endpoint(#[case] region: Region, #[case] expected_endpoint: &str) {
        assert_eq!(
            region.public_key_endpoint().to_string(),
            expected_endpoint.to_string()
        );
    }

    #[rstest]
    #[case(
        Region::US,
        "https://system-identity-oauth.service.newrelic.com/oauth2/token"
    )]
    #[case(
        Region::EU,
        "https://system-identity-oauth.service.newrelic.com/oauth2/token"
    )]
    #[case(
        Region::JP,
        "https://system-identity-oauth.service.newrelic.com/oauth2/token"
    )]
    #[case(
        Region::STAGING,
        "https://system-identity-oauth.staging-service.newrelic.com/oauth2/token"
    )]
    fn test_token_renewal_endpoint(#[case] region: Region, #[case] expected_endpoint: &str) {
        assert_eq!(
            region.token_renewal_endpoint().to_string(),
            expected_endpoint.to_string()
        );
    }

    #[rstest]
    #[case(Region::US, "https://otlp.nr-data.net:4317/")]
    #[case(Region::EU, "https://otlp.eu01.nr-data.net:4317/")]
    #[case(Region::JP, "https://otlp.jp.nr-data.net:4317/")]
    #[case(Region::STAGING, "https://staging-otlp.nr-data.net:4317/")]
    fn test_otel_endpoint(#[case] region: Region, #[case] expected_endpoint: &str) {
        assert_eq!(
            region.otel_endpoint().to_string(),
            expected_endpoint.to_string()
        );
    }
}
