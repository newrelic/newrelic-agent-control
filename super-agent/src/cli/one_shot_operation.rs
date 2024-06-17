use crate::utils::binary_metadata::binary_metadata;

use super::{Cli, CliError};

pub enum OneShotOperation {
    PrintVersion,
    PrintDebugInfo(Cli),
}

impl OneShotOperation {
    /// Runs the one-shot operation
    pub fn run_one_shot(&self) -> Result<(), CliError> {
        match self {
            OneShotOperation::PrintVersion => {
                println!("{}", binary_metadata());
                Ok(())
            }
            OneShotOperation::PrintDebugInfo(cli) => {
                println!("Printing debug info");
                println!("CLI: {:#?}", cli);

                #[cfg(feature = "onhost")]
                println!("Feature: onhost");
                #[cfg(feature = "k8s")]
                println!("Feature: k8s");
                Ok(())
            }
        }
    }
}
