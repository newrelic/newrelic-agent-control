use std::error::Error;
use crowdstrike_sensor::cli::Cli;
use crowdstrike_sensor::crowdstrike::config::SensorOSMappingConfig;
use crowdstrike_sensor::crowdstrike::defaults::CROWDSTRIKE_SENSOR_INSTALLER_HASH_OS_MAPPING;
use crowdstrike_sensor::crowdstrike::installers_getter::{CROWDSTRIKE_INSTALLERS_ENDPOINT, CROWDSTRIKE_TOKEN_ENDPOINT, InstallerGetter};
use crowdstrike_sensor::crowdstrike::os_installer_mapper::OSInstallerMapper;

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::init();
    let installer_getter = InstallerGetter::new(
        cli.client_id,
        cli.client_secret,
        CROWDSTRIKE_TOKEN_ENDPOINT.to_string(),
        CROWDSTRIKE_INSTALLERS_ENDPOINT.to_string(),
    );
    let installers = installer_getter.get_installers()?;

    let config = SensorOSMappingConfig::parse(CROWDSTRIKE_SENSOR_INSTALLER_HASH_OS_MAPPING)?;
    let os_installer_mapper = OSInstallerMapper::new(config);
    println!("{:?}", os_installer_mapper.get_latest_hashes_by_os(installers)?);
    Ok(())
}
