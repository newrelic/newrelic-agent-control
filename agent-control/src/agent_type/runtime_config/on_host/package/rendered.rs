use oci_spec::distribution::Reference;
use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
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
    pub public_key_url: Option<Url>,
}
