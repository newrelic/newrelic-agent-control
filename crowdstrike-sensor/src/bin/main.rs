use std::error::Error;
use crowdstrike_sensor::cli::Cli;
use crowdstrike_sensor::crowdstrike::installers_getter::{CROWDSTRIKE_INSTALLERS_ENDPOINT, CROWDSTRIKE_TOKEN_ENDPOINT, InstallerGetter};

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::init();
    let installer_getter = InstallerGetter::new(
        cli.client_id,
        cli.client_secret,
        CROWDSTRIKE_TOKEN_ENDPOINT.to_string(),
        CROWDSTRIKE_INSTALLERS_ENDPOINT.to_string(),
    );
    println!("{:?}", installer_getter.installers()?);
    Ok(())
}
