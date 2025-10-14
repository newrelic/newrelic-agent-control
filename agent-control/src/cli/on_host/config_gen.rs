use crate::cli::error::CliError;

#[derive(Debug, clap::Parser)]
pub struct ConfigInputs {
    /// Fleet identifier
    #[arg(long)]
    fleet_id: Option<String>,
    // TODO: add remaining inputs
}

/// Generates the Agent Control configuration and any requisite according to the provided inputs.
pub fn generate_config(_inputs: ConfigInputs) -> Result<(), CliError> {
    unimplemented!("TODO: implement config generation")
}
