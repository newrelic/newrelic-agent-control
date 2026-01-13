use crate::agent_type::runtime_config::on_host::package::PackageType;
use oci_spec::distribution::Reference;

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    /// Package type (tar.gz, zip).
    pub package_type: PackageType,

    /// Download defines the supported repository sources for the packages.
    pub download: Download,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Download {
    /// OCI repository definition
    pub oci: Oci,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Oci {
    pub reference: Reference,
}
