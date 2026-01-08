use std::{path::PathBuf, sync::Arc};

use fs::{LocalFile, directory_manager::DirectoryManagerFs};
use newrelic_agent_control::{
    http::config::ProxyConfig,
    package::oci::{downloader::OCIRefDownloader, package_manager::OCIPackageManager},
};
use oci_client::client::{ClientConfig, ClientProtocol};

pub fn new_testing_oci_package_manager(
    base_path: PathBuf,
) -> OCIPackageManager<OCIRefDownloader, DirectoryManagerFs, LocalFile> {
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap(),
    );

    let downloader = OCIRefDownloader::try_new(
        ProxyConfig::default(),
        runtime,
        Some(ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        }),
    )
    .unwrap();

    OCIPackageManager {
        downloader,
        directory_manager: DirectoryManagerFs,
        file_manager: LocalFile,
        base_path,
    }
}
