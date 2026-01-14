use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::runtime_config::templateable_value::TemplateableValue;
use crate::agent_type::templates::Templateable;
use oci_spec::distribution::Reference;
use serde::Deserialize;
use std::str::FromStr;

pub mod rendered;

#[derive(Debug, Deserialize, Clone, PartialEq, Default)]
pub enum PackageType {
    #[default]
    Tar,
    Zip,
}

impl FromStr for PackageType {
    type Err = AgentTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tar" => Ok(Self::Tar),
            "tar.gz" => Ok(Self::Tar),
            "zip" => Ok(Self::Zip),
            _ => Err(AgentTypeError::UnsupportedPackageType(s.to_string())),
        }
    }
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub(super) struct Package {
    #[serde(rename = "type")]
    pub package_type: TemplateableValue<PackageType>, // Using `r#type` to avoid keyword conflict

    /// Download defines the supported repository sources for the packages.
    pub download: Download,
}

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
}

impl Templateable for Package {
    type Output = rendered::Package;
    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        Ok(Self::Output {
            package_type: self.package_type.template_with(variables)?,
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

        if !version.is_empty() && !version.starts_with('@') {
            version = format!(":{}", version);
        }

        let string_reference = format!("{}/{}{}", registry, repository, version);
        let reference = Reference::from_str(string_reference.as_str()).map_err(|err| {
            AgentTypeError::OCIReferenceParsingError(format!(
                "parsing OCI reference {string_reference}: {err}"
            ))
        })?;

        Ok(Self::Output { reference })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::definition::Variables;
    use crate::agent_type::runtime_config::templateable_value::TemplateableValue;
    use crate::agent_type::variable::Variable;
    use rstest::rstest;

    #[test]
    fn test_package_type_from_str() {
        assert_eq!(PackageType::from_str("tar").unwrap(), PackageType::Tar);
        assert_eq!(PackageType::from_str("tar.gz").unwrap(), PackageType::Tar);
        assert_eq!(PackageType::from_str("zip").unwrap(), PackageType::Zip);
        assert!(PackageType::from_str("unsupported").is_err());
    }

    #[rstest]
    #[case::only_digest(
        "@sha256:ec5f08ee7be8b557cd1fc5ae1a0ac985e8538da7c93f51a51eff4b277509a723",
        None,
        Some("sha256:ec5f08ee7be8b557cd1fc5ae1a0ac985e8538da7c93f51a51eff4b277509a723")
    )]
    #[case::tag_and_digest(
        "a-tag@sha256:ec5f08ee7be8b557cd1fc5ae1a0ac985e8538da7c93f51a51eff4b277509a723",
        Some("a-tag"),
        Some("sha256:ec5f08ee7be8b557cd1fc5ae1a0ac985e8538da7c93f51a51eff4b277509a723")
    )]
    #[case::empty_version("", Some("latest"), None)]
    fn oci_template(
        #[case] version: &str,
        #[case] tag: Option<&str>,
        #[case] digest: Option<&str>,
    ) {
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

        let oci = Oci {
            registry: TemplateableValue::from_template("${nr-var:registry}".to_string()),
            repository: TemplateableValue::from_template("${nr-var:repository}".to_string()),
            version: TemplateableValue::from_template("${nr-var:version}".to_string()),
        };

        let rendered_oci = oci.template_with(&variables);
        assert!(rendered_oci.is_ok());

        let rendered_oci = rendered_oci.unwrap();

        assert_eq!(rendered_oci.reference.registry(), "registry.com");
        assert_eq!(rendered_oci.reference.repository(), "repo");
        assert_eq!(rendered_oci.reference.tag(), tag);
        assert_eq!(rendered_oci.reference.digest(), digest);
    }
}
