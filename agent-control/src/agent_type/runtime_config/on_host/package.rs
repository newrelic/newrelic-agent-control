use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::runtime_config::templateable_value::TemplateableValue;
use crate::agent_type::templates::Templateable;
use crate::oci::reference_parser::ReferenceParser;
use oci_client::{Reference, secrets::RegistryAuth};
use serde::Deserialize;
use std::str::FromStr;
use tracing::debug;
use url::Url;

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
    /// Authentication method for the OCI registry.
    #[serde(default)]
    pub auth: Auth,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Auth {
    pub basic: BasicAuth,
    pub bearer: BearerAuth,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct BasicAuth {
    pub username: TemplateableValue<String>,
    pub password: TemplateableValue<String>,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct BearerAuth {
    pub token: TemplateableValue<String>,
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
            .map(|s| Url::parse(&s))
            .transpose()
            .map_err(|err| {
                AgentTypeError::OCIReferenceParsingError(format!("invalid public_key_url: {err}"))
            })?;

        if !version.is_empty() && !version.starts_with('@') {
            version = format!(":{}", version);
        }

        let string_reference = format!("{}/{}{}", registry, repository, version);
        let reference = Reference::from(
            ReferenceParser::from_str(string_reference.as_str()).map_err(|err| {
                AgentTypeError::OCIReferenceParsingError(format!(
                    "parsing OCI reference {string_reference}: {err}"
                ))
            })?,
        );

        let auth = self.auth.template_with(variables)?;

        Ok(Self::Output {
            reference,
            public_key_url,
            auth,
        })
    }
}

impl Templateable for Auth {
    type Output = RegistryAuth;
    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        let username = self
            .basic
            .username
            .template_with(variables)
            .map_err(|err| {
                AgentTypeError::OCIAuthError(format!("error templating username: {err}"))
            })?;
        let password =
            self.basic.password.template_with(variables).map_err(|_| {
                AgentTypeError::OCIAuthError("error templating password".to_string())
            })?;
        let token = self
            .bearer
            .token
            .template_with(variables)
            .map_err(|_| AgentTypeError::OCIAuthError("error templating token".to_string()))?;

        match (
            &username.is_empty(),
            &password.is_empty(),
            &token.is_empty(),
        ) {
            (true, true, true) => Ok(RegistryAuth::Anonymous),
            (false, false, true) => {
                debug!("Basic auth credentials provided, using basic auth");
                Ok(RegistryAuth::Basic(username, password))
            }
            (true, true, false) => {
                debug!("Bearer token provided, using bearer auth");
                Ok(RegistryAuth::Bearer(token))
            }
            (false, false, false) => Err(AgentTypeError::OCIAuthError(
                "multiple authentication methods provided, only one should be used".to_string(),
            )),
            (true, false, _) | (false, true, _) => Err(AgentTypeError::OCIAuthError(
                "incomplete basic auth credentials provided, username or password is empty"
                    .to_string(),
            )),
        }
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
        if let Some(pk) = &public_key_url {
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
            auth: Auth::default(),
        };

        let rendered_oci = oci.template_with(&variables);
        let rendered_oci = rendered_oci.unwrap();

        assert_eq!(rendered_oci.reference.registry(), "registry.com");
        assert_eq!(rendered_oci.reference.repository(), "repo");
        assert_eq!(rendered_oci.reference.tag(), expected_tag);
        assert_eq!(rendered_oci.reference.digest(), expected_digest);
        assert_eq!(rendered_oci.public_key_url, public_key_url);
        assert_eq!(rendered_oci.auth, RegistryAuth::Anonymous);
    }

    #[test]
    fn test_auth_basic_with_templated_values() {
        let mut variables = Variables::new();
        variables.insert(
            "nr-var:username".to_string(),
            Variable::new_final_string_variable("myuser".to_string()),
        );
        variables.insert(
            "nr-var:password".to_string(),
            Variable::new_final_string_variable("mypass".to_string()),
        );

        let oci = Oci {
            registry: TemplateableValue::from_template("docker.io".to_string()),
            repository: TemplateableValue::from_template("myrepo/myimage".to_string()),
            version: TemplateableValue::from_template("1.0.0".to_string()),
            public_key_url: None,
            auth: Auth {
                basic: BasicAuth {
                    username: TemplateableValue::from_template("${nr-var:username}".to_string()),
                    password: TemplateableValue::from_template("${nr-var:password}".to_string()),
                },
                bearer: BearerAuth::default(),
            },
        };

        let rendered_oci = oci.template_with(&variables).unwrap();
        assert_eq!(
            rendered_oci.auth,
            RegistryAuth::Basic("myuser".to_string(), "mypass".to_string())
        );
    }

    #[test]
    fn test_auth_bearer_with_templated_value() {
        let mut variables = Variables::new();
        variables.insert(
            "nr-var:token".to_string(),
            Variable::new_final_string_variable("bearer-token".to_string()),
        );

        let oci = Oci {
            registry: TemplateableValue::from_template("gcr.io".to_string()),
            repository: TemplateableValue::from_template("myproject/myimage".to_string()),
            version: TemplateableValue::from_template("latest".to_string()),
            public_key_url: None,
            auth: Auth {
                basic: BasicAuth::default(),
                bearer: BearerAuth {
                    token: TemplateableValue::from_template("${nr-var:token}".to_string()),
                },
            },
        };

        let rendered_oci = oci.template_with(&variables).unwrap();
        assert_eq!(
            rendered_oci.auth,
            RegistryAuth::Bearer("bearer-token".to_string())
        );
    }

    #[rstest]
    #[case::multiple_credentials_provided("myuser", "mypass", "bearer-token")]
    #[case::no_username_in_basic_auth("", "mypass", "")]
    #[case::no_password_in_basic_auth("myuser", "", "")]
    fn test_auth_credentials_error(
        #[case] username: String,
        #[case] password: String,
        #[case] token: String,
    ) {
        let mut variables = Variables::new();
        variables.insert(
            "nr-var:username".to_string(),
            Variable::new_final_string_variable(username),
        );
        variables.insert(
            "nr-var:password".to_string(),
            Variable::new_final_string_variable(password),
        );
        variables.insert(
            "nr-var:token".to_string(),
            Variable::new_final_string_variable(token),
        );

        let oci = Oci {
            registry: TemplateableValue::from_template("gcr.io".to_string()),
            repository: TemplateableValue::from_template("myproject/myimage".to_string()),
            version: TemplateableValue::from_template("latest".to_string()),
            public_key_url: None,
            auth: Auth {
                basic: BasicAuth {
                    username: TemplateableValue::from_template("${nr-var:username}".to_string()),
                    password: TemplateableValue::from_template("${nr-var:password}".to_string()),
                },
                bearer: BearerAuth {
                    token: TemplateableValue::from_template("${nr-var:token}".to_string()),
                },
            },
        };

        let rendered_oci = oci.template_with(&variables);
        matches!(rendered_oci, Err(AgentTypeError::OCIAuthError(_)));
    }
}
