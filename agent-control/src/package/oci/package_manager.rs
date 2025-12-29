use std::{convert::Infallible, path::PathBuf};

use oci_client::Reference;

use crate::package::manager::PackageManager;

use super::downloader::OCIDownloader;

pub struct OCIPackageManager {
    pkg_downloader: OCIDownloader,
    base_path: PathBuf,
}

impl PackageManager for OCIPackageManager {
    type Error = Infallible; // TODO change

    type Package = Reference; // TODO review

    type InstalledPackage = PathBuf; // Downloaded package location

    fn install(
        agent_id: &crate::agent_control::agent_id::AgentID,
        package: Self::Package,
    ) -> Result<Self::InstalledPackage, Self::Error> {
        todo!()
    }

    fn uninstall(
        agent_id: &crate::agent_control::agent_id::AgentID,
        package: Self::InstalledPackage,
    ) -> Result<(), Self::Error> {
        todo!("Only installation for now")
    }
}
