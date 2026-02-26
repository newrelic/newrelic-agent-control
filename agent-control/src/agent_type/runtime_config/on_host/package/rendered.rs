use oci_client::Reference;
use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    /// Download defines the supported repository sources for the packages.
    pub download: Download,
    /// Optional preinstall script to run before package installation
    pub preinstall: Option<InstallScript>,
    /// Optional postinstall script to run after package extraction
    pub postinstall: Option<InstallScript>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstallScript {
    /// Path to shell script file relative to the extracted package directory
    pub script_path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Download {
    /// OCI repository definition
    pub oci: Oci,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Oci {
    pub reference: Reference,
    pub public_key_url: Option<Url>,
}
