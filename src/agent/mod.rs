use std::fmt::{Display, Formatter, Result};

use self::error::AgentError;

mod error;

pub mod logging;

impl Display for AgentError {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(f, "invalid first item to double")
    }
}
