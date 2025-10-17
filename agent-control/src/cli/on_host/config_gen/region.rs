//! This module defines configuration related to region.
use clap::builder::ValueParser;
use http::Uri;
use nr_auth::{
    parameters::Environments, system_identity::input_data::environment::NewRelicEnvironment,
};

const OPAMP_ENDPOINT_US: &str = "https://opamp.service.newrelic.com/v1/opamp";
const OPAMP_ENDPOINT_EU: &str = "https://opamp.service.eu.newrelic.com/v1/opamp";
const OPAMP_ENDPOINT_STAGING: &str = "https://staging-service.newrelic.com/v1/opamp";

const PUBLIC_KEY_ENDPOINT_US: &str =
    "https://publickeys.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json";
const PUBLIC_KEY_ENDPOINT_EU: &str =
    "https://publickeys.eu.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json";
const PUBLIC_KEY_ENDPOINT_STAGING: &str =
    "https://staging-publickeys.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json";

/// Represents the supported region and defines related fields. It cannot wrap the [Environments] enum
/// due to clap limitations. Re-defining the enum is simpler than extending and using some mapping
/// tool such as [clap::builder::TypedValueParser::map].
#[derive(Debug, Copy, Clone, PartialEq, clap::ValueEnum)]
pub enum Region {
    US,
    EU,
    STAGING,
}

pub fn region_parser() -> ValueParser {
    ValueParser::from(|s: &str| -> Result<Region, String> {
        match s {
            s if s.to_lowercase() == "us" => Ok(Region::US),
            s if s.to_lowercase() == "eu" => Ok(Region::EU),
            s if s.to_lowercase() == "staging" => Ok(Region::STAGING),
            _ => Err(format!("{s} is not a supported region")),
        }
    })
}

impl From<Region> for Environments {
    fn from(value: Region) -> Self {
        match value {
            Region::US => Environments::US,
            Region::EU => Environments::EU,
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
    pub fn opamp_endpoint(&self) -> Uri {
        match &self {
            Self::US => OPAMP_ENDPOINT_US,
            Self::EU => OPAMP_ENDPOINT_EU,
            Self::STAGING => OPAMP_ENDPOINT_STAGING,
        }
        .parse()
        .expect("known uris should be valid")
    }

    pub fn public_key_endpoint(&self) -> Uri {
        match &self {
            Self::US => PUBLIC_KEY_ENDPOINT_US,
            Self::EU => PUBLIC_KEY_ENDPOINT_EU,
            Self::STAGING => PUBLIC_KEY_ENDPOINT_STAGING,
        }
        .parse()
        .expect("known uris should be valid")
    }

    // Helper to obtain the token renewal endpoint that is also needed in Agent Control configuration
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
    #[case(Region::STAGING, Environments::STAGING)]
    fn test_environments_conversion(#[case] region: Region, #[case] expected: Environments) {
        assert_eq!(Environments::from(region), expected);
    }

    #[rstest]
    #[case(Region::US, "https://opamp.service.newrelic.com/v1/opamp")]
    #[case(Region::EU, "https://opamp.service.eu.newrelic.com/v1/opamp")]
    #[case(Region::STAGING, "https://staging-service.newrelic.com/v1/opamp")]
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
    #[case("us", Region::US)]
    #[case("US", Region::US)]
    #[case("eu", Region::EU)]
    #[case("EU", Region::EU)]
    #[case("staging", Region::STAGING)]
    #[case("STAGING", Region::STAGING)]
    fn test_region_parser_ok(#[case] input: &str, #[case] expected: Region) {
        let cmd = clap::Command::new("cmd").arg(
            clap::Arg::new("flag")
                .long("flag")
                .value_parser(region_parser()),
        );
        let matches = cmd.try_get_matches_from(["cmd", "--flag", input]).unwrap();
        let value = matches.get_one::<Region>("flag").cloned();
        assert_eq!(value, Some(expected));
    }

    #[rstest]
    #[case("invalid")]
    #[case("")]
    //#[case("invalid", Err("invalid is not a supported region".to_string()))]
    //#[case("", Err(" is not a supported region".to_string()))]
    fn test_region_parser_error(#[case] input: &str) {
        let cmd = clap::Command::new("cmd").arg(
            clap::Arg::new("flag")
                .long("flag")
                .value_parser(region_parser()),
        );
        let err = cmd
            .try_get_matches_from(["cmd", "--flag", input])
            .unwrap_err();
        assert!(err.to_string().contains(input))
    }
}
