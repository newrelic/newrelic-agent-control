use oci_client::Reference;
use std::path::PathBuf;
use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    /// Download defines the supported repository sources for the packages.
    pub download: Download,
    /// Post-install hooks to execute after package extraction.
    pub post_install: Vec<PostInstallHook>,
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

/// Rendered post-install hook with all templates resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct PostInstallHook {
    pub action: PostInstallAction,
}

/// Rendered post-install actions with resolved paths.
#[derive(Debug, Clone, PartialEq)]
pub enum PostInstallAction {
    Copy {
        source: PathBuf,
        destination: PathBuf,
        create_parent_dirs: bool,
    },
    Symlink {
        source: PathBuf,
        destination: PathBuf,
        create_parent_dirs: bool,
    },
}
