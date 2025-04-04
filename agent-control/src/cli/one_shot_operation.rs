use super::Cli;
use crate::utils::binary_metadata::binary_metadata;

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
            }
        }
    }
}
