use flate2::Compression;
use flate2::write::GzEncoder;
use fs::directory_manager::DirectoryManagerFs;
use newrelic_agent_control::{
    http::config::ProxyConfig,
    package::oci::{downloader::OCIRefDownloader, package_manager::OCIPackageManager},
};
use oci_client::client::{ClientConfig, ClientProtocol};
use std::fs::File;
use std::path::Path;
use std::{path::PathBuf, sync::Arc};

pub fn new_testing_oci_package_manager(
    base_path: PathBuf,
) -> OCIPackageManager<OCIRefDownloader, DirectoryManagerFs> {
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
        base_path,
    }
}

pub fn compress_tar_gz(source_path: &Path, tmp_file_archive: &Path) {
    let tar_gz = File::create(tmp_file_archive).unwrap();
    let enc = GzEncoder::new(tar_gz, Compression::default());
    let mut tar = tar::Builder::new(enc);
    tar.append_dir_all(".", source_path).unwrap();
    tar.finish().unwrap();
}

pub fn create_data_to_compress(tmp_dir_to_compress: &Path) {
    let file_path_1 = tmp_dir_to_compress.join("file1.txt");
    File::create(file_path_1.clone()).unwrap();
    let file_path_2 = tmp_dir_to_compress.join("file2.txt");
    File::create(file_path_2.clone()).unwrap();

    std::fs::write(file_path_1.as_path(), "important content").unwrap();
    std::fs::write(file_path_2.as_path(), "important content").unwrap();
}
