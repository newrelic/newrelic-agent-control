use oci_client::Reference;
use std::collections::HashMap;
use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    /// Download defines the supported repository sources for the packages.
    pub download: Download,
    /// Optional postdownload script to run after package extraction
    pub postdownload: Option<PostDownloadScript>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PostDownloadScript {
    /// Path to script file relative to the extracted package directory
    pub script_path: String,
    /// Command/binary to execute the script
    pub command: String,
    /// Arguments to pass to the script
    pub args: Vec<String>,
    /// Environment variables to pass to the script process
    pub env: HashMap<String, String>,
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
