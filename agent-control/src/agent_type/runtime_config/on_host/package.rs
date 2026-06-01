use std::collections::HashMap;
use std::str::FromStr;

use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::runtime_config::on_host::package::rendered::{Repository, Version};
use crate::agent_type::runtime_config::templateable_value::TemplateableValue;
use crate::agent_type::templates::Templateable;
use serde::Deserialize;
use url::Url;

pub mod rendered;

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub(super) struct Package {
    /// Download defines the supported repository sources for the packages.
    pub download: Download,
    /// Postdownload script to execute after downloading and extracting the package.
    /// All validations, checks, and installation steps should go here.
    pub postdownload: Option<Postdownload>,
}

pub type PackageID = String;

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Download {
    /// OCI repository definition
    pub oci: Oci,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Oci {
    /// Repository name.
    pub repository: TemplateableValue<String>,
    /// Package version including tag, digest or tag + digest.
    #[serde(default)]
    pub version: TemplateableValue<String>,
    /// Public key url is expected to be a jwks.
    pub public_key_url: Option<TemplateableValue<String>>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Postdownload {
    /// Arguments where first element is the command/executable (e.g., "bash", "sh", "python3"),
    /// second element is the script path, followed by additional arguments.
    /// Example: ["bash", "postdownload.sh", "--verbose"]
    pub args: Vec<TemplateableValue<String>>,

    /// Environmental variables passed to the process.
    #[serde(default)]
    pub env: HashMap<String, TemplateableValue<String>>,

    /// Maximum time to wait for the script to complete.
    #[serde(default = "default_postdownload_timeout")]
    pub timeout: TemplateableValue<String>,
}

fn default_postdownload_timeout() -> TemplateableValue<String> {
    TemplateableValue::new("300s".to_string())
}

impl Templateable for Package {
    type Output = rendered::Package;
    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        let postdownload = self
            .postdownload
            .map(|pd| pd.template_with(variables))
            .transpose()?;

        Ok(Self::Output {
            download: self.download.template_with(variables)?,
            postdownload,
        })
    }
}

impl Templateable for Download {
    type Output = rendered::Download;
    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        Ok(Self::Output {
            oci: self.oci.template_with(variables)?,
        })
    }
}

impl Templateable for Oci {
    type Output = rendered::Oci;
    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        let repository =
            Repository::from_str(&self.repository.template_with(variables)?).map_err(|err| {
                AgentTypeError::OCIReferenceParsingError(format!("invalid repository: {err}"))
            })?;

        let version =
            Version::from_str(&self.version.template_with(variables)?).map_err(|err| {
                AgentTypeError::OCIReferenceParsingError(format!("invalid version: {err}"))
            })?;

        let public_key_url = self
            .public_key_url
            .map(|pk| pk.template_with(variables))
            .transpose()?;

        let public_key_url = public_key_url
            .map(|s| Url::parse(&s))
            .transpose()
            .map_err(|err| {
                AgentTypeError::OCIReferenceParsingError(format!("invalid public_key_url: {err}"))
            })?;

        Ok(Self::Output {
            repository,
            version,
            public_key_url,
        })
    }
}

impl Templateable for Postdownload {
    type Output = rendered::Postdownload;
    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        let timeout_str = self.timeout.template_with(variables)?;

        let args: Vec<String> = self
            .args
            .into_iter()
            .map(|arg| arg.template_with(variables))
            .collect::<Result<Vec<String>, AgentTypeError>>()?;

        if args.len() < 2 {
            return Err(AgentTypeError::OCIReferenceParsingError(
                "postdownload args must have at least 2 elements: command and script path"
                    .to_string(),
            ));
        }

        let env: HashMap<String, String> = self
            .env
            .into_iter()
            .map(|(k, v)| v.template_with(variables).map(|templated| (k, templated)))
            .collect::<Result<HashMap<_, _>, AgentTypeError>>()?;

        let timeout = duration_str::parse(&timeout_str).map_err(|err| {
            AgentTypeError::OCIReferenceParsingError(format!("invalid timeout format: {err}"))
        })?;

        Ok(Self::Output { args, env, timeout })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::agent_type::definition::Variables;
    use crate::agent_type::runtime_config::on_host::package::rendered::Repository;
    use crate::agent_type::runtime_config::templateable_value::TemplateableValue;
    use crate::agent_type::variable::Variable;
    use rstest::rstest;
    use url::Url;

    #[rstest]
    #[case::with_public_key_url(Some("https://github.com/rust-lang/crates.io-index".parse().unwrap()))]
    #[case::without_public_key_url(None)]
    fn test_oci_template(#[case] public_key_url: Option<Url>) {
        use crate::agent_type::runtime_config::on_host::package::rendered::Version;

        let version =
            "a-tag@sha256:ec5f08ee7be8b557cd1fc5ae1a0ac985e8538da7c93f51a51eff4b277509a723"
                .to_string();

        let mut variables = Variables::new();
        variables.insert(
            "nr-var:repository".to_string(),
            Variable::new_final_string_variable("repo".to_string()),
        );
        variables.insert(
            "nr-var:version".to_string(),
            Variable::new_final_string_variable(version.clone()),
        );
        if let Some(pk) = &public_key_url {
            variables.insert(
                "nr-var:public-key".to_string(),
                Variable::new_final_string_variable(pk.to_string()),
            );
        }

        let oci = Oci {
            repository: TemplateableValue::from_template("${nr-var:repository}".to_string()),
            version: TemplateableValue::from_template("${nr-var:version}".to_string()),
            public_key_url: public_key_url
                .clone()
                .map(|_| TemplateableValue::from_template("${nr-var:public-key}".to_string())),
        };

        let rendered_oci = oci.template_with(&variables).unwrap();
        assert_eq!(
            rendered_oci.repository,
            Repository::from_str("repo").unwrap()
        );
        assert_eq!(rendered_oci.version, Version::from_str(&version).unwrap());
        assert_eq!(rendered_oci.public_key_url, public_key_url);
    }
}
