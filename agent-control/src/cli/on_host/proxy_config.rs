//!  This module defines the proxy configuration for on-hots commands.

use std::convert::Infallible;

use serde::Serialize;

/// Holds the proxy configuration.
/// Cannot use [crate::http::config::ProxyConfig] directly due lack of support for defaults in clap.
/// See <https://github.com/clap-rs/clap/issues/4746> for details.
#[derive(Debug, Default, Clone, PartialEq, Serialize, clap::Args)]
pub struct ProxyConfig {
    #[serde(skip_serializing_if = "is_none_or_empty_string", rename = "url")]
    #[arg(long, required = false)]
    pub proxy_url: Option<String>,

    #[serde(
        skip_serializing_if = "is_none_or_empty_string",
        rename = "ca_bundle_dir"
    )]
    #[arg(long, required = false)]
    pub proxy_ca_bundle_dir: Option<String>,

    #[serde(
        skip_serializing_if = "is_none_or_empty_string",
        rename = "ca_bundle_file"
    )]
    #[arg(long, required = false)]
    pub proxy_ca_bundle_file: Option<String>,

    /// Note that if you do not want to provide a value you still need to pass '--ignore-system-proxy ""' or '--ignore-system-proxy='
    #[serde(skip_serializing_if = "is_false")]
    #[arg(long, default_value_t = false, value_parser = ignore_system_proxy_parser, action = clap::ArgAction::Set)]
    pub ignore_system_proxy: bool,
}

impl ProxyConfig {
    pub fn is_empty(&self) -> bool {
        [
            self.proxy_url.as_ref(),
            self.proxy_ca_bundle_dir.as_ref(),
            self.proxy_ca_bundle_file.as_ref(),
        ]
        .iter()
        .all(|v| v.is_none_or(|s| s.is_empty()))
            && !self.ignore_system_proxy
    }
}

// Helper to avoid serializing empty values
fn is_none_or_empty_string(v: &Option<String>) -> bool {
    v.as_ref().is_none_or(|s| s.is_empty())
}

// Helper to avoid serializing false values
fn is_false(v: &bool) -> bool {
    !v
}

// Custom parser to allow empty values as false booleans
fn ignore_system_proxy_parser(s: &str) -> Result<bool, Infallible> {
    match s.to_lowercase().as_str() {
        "" | "false" => Ok(false),
        _ => Ok(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{Args, FromArgMatches};
    use rstest::rstest;

    #[test]
    fn test_serialize_proxy_config_all_empty_options() {
        let proxy_config = ProxyConfig {
            proxy_url: Some(String::new()),
            proxy_ca_bundle_dir: Some(String::new()),
            proxy_ca_bundle_file: Some(String::new()),
            ignore_system_proxy: false,
        };

        let serialized = serde_yaml::to_string(&proxy_config).unwrap();
        assert_eq!(serialized.trim(), "{}");
    }

    #[test]
    fn test_serialize_proxy_config_none_options() {
        let proxy_config = ProxyConfig {
            proxy_url: None,
            proxy_ca_bundle_dir: None,
            proxy_ca_bundle_file: None,
            ignore_system_proxy: true,
        };

        let serialized = serde_yaml::to_string(&proxy_config).unwrap();
        // Only ignore_system_proxy should be present
        assert_eq!(serialized.trim(), "ignore_system_proxy: true");
    }

    #[rstest]
    #[case("", ProxyConfig::default())]
    #[case(
        "--proxy-url https://proxy.url --proxy-ca-bundle-dir=/bundle/dir --proxy-ca-bundle-file=/bundle/file --ignore-system-proxy true",
        ProxyConfig{proxy_url: Some("https://proxy.url".into()), proxy_ca_bundle_dir: Some("/bundle/dir".into()), proxy_ca_bundle_file: Some("/bundle/file".into()), ignore_system_proxy: true},
    )]
    #[case("--proxy-url= --proxy-ca-bundle-dir= --proxy-ca-bundle-file= --ignore-system-proxy=", ProxyConfig{proxy_url: Some("".into()), proxy_ca_bundle_dir: Some("".into()), proxy_ca_bundle_file: Some("".into()), ignore_system_proxy: false})]
    #[case(" --ignore-system-proxy=", ProxyConfig{ignore_system_proxy: false, ..Default::default()})]
    #[case(" --ignore-system-proxy=false", ProxyConfig{ignore_system_proxy: false, ..Default::default()})]
    #[case(" --ignore-system-proxy=true", ProxyConfig{ignore_system_proxy: true, ..Default::default()})]
    #[case(" --ignore-system-proxy=False", ProxyConfig{ignore_system_proxy: false, ..Default::default()})]
    #[case(" --ignore-system-proxy=True", ProxyConfig{ignore_system_proxy: true, ..Default::default()})]
    fn test_proxy_args(#[case] args: &str, #[case] expected: ProxyConfig) {
        let cmd = clap::Command::new("test").no_binary_name(true);
        let cmd = ProxyConfig::augment_args(cmd);
        let matches = cmd
            .try_get_matches_from(args.split_ascii_whitespace())
            .expect("arguments should be valid");
        let value = ProxyConfig::from_arg_matches(&matches).expect("should create the struct back");
        assert_eq!(value, expected)
    }
}
