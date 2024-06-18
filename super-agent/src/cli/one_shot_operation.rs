use crate::utils::binary_metadata::binary_metadata;

use super::Cli;

pub enum OneShotCommand {
    PrintVersion,
    PrintDebugInfo(Cli),
}

impl OneShotCommand {
    /// Runs the one-shot operation
    pub fn run_one_shot(&self) {
        match self {
            OneShotCommand::PrintVersion => {
                println!("{}", binary_metadata());
            }
            OneShotCommand::PrintDebugInfo(cli) => {
                println!("Printing debug info");
                println!("CLI: {:#?}", cli);

                #[cfg(feature = "onhost")]
                println!("Feature: onhost");
                #[cfg(feature = "k8s")]
                println!("Feature: k8s");
            }
        }
    }
}
