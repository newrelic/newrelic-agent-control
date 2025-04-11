use super::Cli;
use crate::{agent_control::run::Environment, utils::binary_metadata::binary_metadata};

pub enum OneShotCommand {
    PrintVersion,
    PrintDebugInfo(Cli),
}

impl OneShotCommand {
    /// Runs the one-shot operation
    pub fn run_one_shot(&self, env: Environment) {
        match self {
            OneShotCommand::PrintVersion => {
                println!("{}", binary_metadata(env));
            }
            OneShotCommand::PrintDebugInfo(cli) => {
                println!("Printing debug info");
                println!("Agent Control Mode: {env:?}");
                println!("CLI: {:#?}", cli);
            }
        }
    }
}
