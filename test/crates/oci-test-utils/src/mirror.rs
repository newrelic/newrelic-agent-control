use crate::{PackageMediaType, PackagePublisher};
use oci_client::Reference;
use oci_client::client::Client;
use oci_client::config::{Architecture, Os};
use oci_client::manifest::OciManifest;
use oci_client::secrets::RegistryAuth;
use std::error::Error;
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::runtime::Handle;

/// Copies the package of the host platform behind `source` into the registry `destination`
/// publishes to, under `tag`. Signatures are not copied.
pub fn mirror_host_package(
    runtime_handle: &Handle,
    source: &Reference,
    destination: &PackagePublisher,
    tag: &str,
) -> Result<Reference, Box<dyn Error>> {
    let client = Client::default();
    let auth = RegistryAuth::Anonymous;

    let (manifest, _) = runtime_handle.block_on(client.pull_manifest(source, &auth))?;
    let OciManifest::ImageIndex(index) = manifest else {
        return Err(format!("'{source}' is not an image index").into());
    };
    let host_entry = index
        .manifests
        .iter()
        .find(|entry| {
            entry.platform.as_ref().is_some_and(|platform| {
                platform.os == Os::default() && platform.architecture == Architecture::default()
            })
        })
        .ok_or_else(|| format!("'{source}' has no manifest for the host platform"))?;

    let platform_reference = Reference::with_digest(
        source.registry().to_string(),
        source.repository().to_string(),
        host_entry.digest.clone(),
    );
    let (image_manifest, _) =
        runtime_handle.block_on(client.pull_image_manifest(&platform_reference, &auth))?;
    let layer = image_manifest
        .layers
        .first()
        .ok_or_else(|| format!("'{source}' has no layers"))?;
    let media_type = if layer.media_type.ends_with(".zip") || layer.media_type.ends_with("+zip") {
        PackageMediaType::Zip
    } else {
        PackageMediaType::TarGz
    };

    let package_file = NamedTempFile::new()?;
    runtime_handle.block_on(async {
        let mut out = File::create(package_file.path()).await?;
        client
            .pull_blob(&platform_reference, layer, &mut out)
            .await?;
        Ok::<_, Box<dyn Error>>(())
    })?;

    Ok(destination.push_with_tag(package_file.path(), media_type, tag))
}
