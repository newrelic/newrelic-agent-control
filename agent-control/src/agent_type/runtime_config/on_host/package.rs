use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::runtime_config::templateable_value::TemplateableValue;
use crate::agent_type::templates::Templateable;
use oci_spec::distribution::Reference;
use serde::Deserialize;
use std::str::FromStr;

pub mod rendered;

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub(super) struct Package {
    /// Download defines the supported repository sources for the packages.
    pub download: Download,
}

pub type PackageID = String;

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Download {
    /// OCI repository definition
    pub oci: Oci,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Oci {
    /// OCI registry url.
    pub registry: TemplateableValue<String>,
    /// Repository name.
    pub repository: TemplateableValue<String>,
    /// Package version including tag, digest or tag + digest.
    #[serde(default)]
    pub version: TemplateableValue<String>,
    /// Public key url is expected to be a jwks.
    pub public_key_url: Option<TemplateableValue<String>>,
}

impl Templateable for Package {
    type Output = rendered::Package;
    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        Ok(Self::Output {
            download: self.download.template_with(variables)?,
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
        let registry = self.registry.template_with(variables)?;
        let repository = self.repository.template_with(variables)?;
        let mut version = self.version.template_with(variables)?;

        let public_key_url = self
            .public_key_url
            .map(|pk| pk.template_with(variables))
            .transpose()?;

        let public_key_url = public_key_url
            .map(|s| s.parse())
            .transpose()
            .map_err(|err| {
                AgentTypeError::OCIReferenceParsingError(format!("invalid public_key_url: {err}"))
            })?;

        if !version.is_empty() && !version.starts_with('@') {
            version = format!(":{}", version);
        }

        let string_reference = format!("{}/{}{}", registry, repository, version);
        let reference = Reference::from_str(string_reference.as_str()).map_err(|err| {
            AgentTypeError::OCIReferenceParsingError(format!(
                "parsing OCI reference {string_reference}: {err}"
            ))
        })?;

        Ok(Self::Output {
            reference,
            public_key_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::definition::Variables;
    use crate::agent_type::runtime_config::templateable_value::TemplateableValue;
    use crate::agent_type::variable::Variable;
    use rstest::rstest;
    use url::Url;

    #[rstest]
    #[case::digest_and_public_key_url(
        "@sha256:ec5f08ee7be8b557cd1fc5ae1a0ac985e8538da7c93f51a51eff4b277509a723",
        Some("https://github.com/rust-lang/crates.io-index".parse().unwrap())
    )]
    #[case::tag_and_public_key_url("a-tag", Some("https://github.com/rust-lang/crates.io-index".parse().unwrap()))]
    #[case::full_version_and_public_key_url(
        "a-tag@sha256:ec5f08ee7be8b557cd1fc5ae1a0ac985e8538da7c93f51a51eff4b277509a723",
        Some("https://github.com/rust-lang/crates.io-index".parse().unwrap())
    )]
    #[case::empty_version_and_public_key_url(
        "",
        Some("https://github.com/rust-lang/crates.io-index".parse().unwrap())
    )]
    #[case::no_version_and_no_public_key_url("", None)]
    fn oci_template(#[case] version: &str, #[case] public_key_url: Option<Url>) {
        let (expected_tag, expected_digest) = if version.is_empty() {
            (Some("latest"), None)
        } else {
            let parts: Vec<&str> = version.splitn(2, '@').collect();
            match parts.as_slice() {
                ["", digest] => (None, Some(*digest)),        // Case: @digest
                [tag, digest] => (Some(*tag), Some(*digest)), // Case: tag@digest
                [tag] => (Some(*tag), None),                  // Case: tag
                _ => (None, None),
            }
        };

        let mut variables = Variables::new();
        variables.insert(
            "nr-var:registry".to_string(),
            Variable::new_final_string_variable("registry.com".to_string()),
        );
        variables.insert(
            "nr-var:repository".to_string(),
            Variable::new_final_string_variable("repo".to_string()),
        );
        variables.insert(
            "nr-var:version".to_string(),
            Variable::new_final_string_variable(version.to_string()),
        );
        if let Some(pk) = public_key_url.clone() {
            variables.insert(
                "nr-var:public-key".to_string(),
                Variable::new_final_string_variable(pk.to_string()),
            );
        }

        let oci = Oci {
            registry: TemplateableValue::from_template("${nr-var:registry}".to_string()),
            repository: TemplateableValue::from_template("${nr-var:repository}".to_string()),
            version: TemplateableValue::from_template("${nr-var:version}".to_string()),
            public_key_url: public_key_url
                .clone()
                .map(|_| TemplateableValue::from_template("${nr-var:public-key}".to_string())),
        };

        let rendered_oci = oci.template_with(&variables);
        let rendered_oci = rendered_oci.unwrap();

        assert_eq!(rendered_oci.reference.registry(), "registry.com");
        assert_eq!(rendered_oci.reference.repository(), "repo");
        assert_eq!(rendered_oci.reference.tag(), expected_tag);
        assert_eq!(rendered_oci.reference.digest(), expected_digest);
        assert_eq!(rendered_oci.public_key_url, public_key_url);
    }
}
