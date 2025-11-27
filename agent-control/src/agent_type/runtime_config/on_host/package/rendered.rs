use crate::agent_type::runtime_config::on_host::package::PackageType;
use oci_spec::distribution::Reference;

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    /// Package type (tar.gz, zip).
    pub(super) package_type: PackageType,

    /// Download defines the supported repository sources for the packages.
    pub(super) download: Download,
    //TODO: implement signature, install and uninstall fields when defined.
}

#[derive(Debug, Clone, PartialEq)]
pub struct Download {
    /// OCI repository definition
    pub(super) oci: Oci,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Oci {
    pub(super) reference: Reference,
}
