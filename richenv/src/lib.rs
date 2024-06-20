use thiserror::Error;

use crate::definition::EnvVarsDefinition;
use crate::envvar::EnvVars;

mod definition;
mod envvar;

#[derive(Error, Debug, Clone)]
pub enum RichEnvError {
    #[error("error populating env vars: `{0}`")]
    PopulatingError(String),
}

#[allow(dead_code)]
trait RichEnv {
    fn populate(self, env_vars: EnvVarsDefinition) -> Result<EnvVars, RichEnvError>;
}
