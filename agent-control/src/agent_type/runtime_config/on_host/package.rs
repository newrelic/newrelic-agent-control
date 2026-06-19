use std::str::FromStr;

use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::runtime_config::on_host::executable::{Args, Env};
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
    /// Post-download hook script to execute after downloading and extracting the package.
    /// All validations, checks, and installation steps should go here.
    pub post_download_hook: Option<PostDownloadHook>,
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
pub struct PostDownloadHook {
    /// Path to the command/executable to run.
    /// Can be an absolute path (e.g., "/bin/bash") or a command name to search in PATH (e.g., "bash").
    /// Supports shell interpreters or direct binaries.
    pub path: TemplateableValue<String>,

    /// Arguments passed to the command.
    /// - When using a shell: first arg is typically the script path, followed by script arguments
    ///   Example: ["/path/to/install.sh", "--check-dependencies", "--verbose"]
    /// - When using a binary directly: arguments for that binary (can be empty)
    ///   Example: ["--flag", "value"] or []
    #[serde(default)]
    pub args: Args,

    /// Environmental variables passed to the process.
    #[serde(default)]
    pub env: Env,
}

impl Templateable for Package {
    type Output = rendered::Package;
    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        let post_download_hook = self
            .post_download_hook
            .map(|pd| pd.template_with(variables))
            .transpose()?;

        Ok(Self::Output {
            download: self.download.template_with(variables)?,
            post_download_hook,
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

impl Templateable for PostDownloadHook {
    type Output = rendered::PostDownloadHook;
    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        let path = self.path.template_with(variables)?;
        let args = self.args.template_with(variables)?;
        let env = self.env.template_with(variables)?;

        Ok(Self::Output { path, args, env })
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

    #[test]
    fn test_post_download_hook_template_with_variables() {
        use std::collections::HashMap;

        let mut variables = Variables::new();
        variables.insert(
            "nr-var:version".to_string(),
            Variable::new_final_string_variable("1.0.0".to_string()),
        );
        variables.insert(
            "nr-var:script-path".to_string(),
            Variable::new_final_string_variable("/opt/install.sh".to_string()),
        );
        variables.insert(
            "nr-var:env-value".to_string(),
            Variable::new_final_string_variable("test-value".to_string()),
        );

        let mut env_map = HashMap::new();
        env_map.insert(
            "AGENT_VERSION".to_string(),
            TemplateableValue::from_template("${nr-var:version}".to_string()),
        );
        env_map.insert(
            "CUSTOM_VAR".to_string(),
            TemplateableValue::from_template("${nr-var:env-value}".to_string()),
        );

        let post_download_hook = PostDownloadHook {
            path: TemplateableValue::from_template("/bin/bash".to_string()),
            args: Args(vec![
                TemplateableValue::from_template("${nr-var:script-path}".to_string()),
                TemplateableValue::from_template("--version=${nr-var:version}".to_string()),
            ]),
            env: Env(env_map),
        };

        let rendered = post_download_hook.template_with(&variables).unwrap();

        assert_eq!(rendered.path, "/bin/bash");
        assert_eq!(rendered.args.0.len(), 2);
        assert_eq!(rendered.args.0[0], "/opt/install.sh");
        assert_eq!(rendered.args.0[1], "--version=1.0.0");
        assert_eq!(rendered.env.0.len(), 2);
        assert_eq!(
            rendered.env.0.get("AGENT_VERSION"),
            Some(&"1.0.0".to_string())
        );
        assert_eq!(
            rendered.env.0.get("CUSTOM_VAR"),
            Some(&"test-value".to_string())
        );
    }
}
