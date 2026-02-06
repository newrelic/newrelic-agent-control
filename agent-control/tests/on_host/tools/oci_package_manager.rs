use flate2::Compression;
use flate2::write::GzEncoder;
use fs::directory_manager::DirectoryManagerFs;
use newrelic_agent_control::{
    http::config::ProxyConfig,
    package::oci::{downloader::OCIArtifactDownloader, package_manager::OCIPackageManager},
};
use oci_client::client::{ClientConfig, ClientProtocol};
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use zip::write::SimpleFileOptions;

use crate::common::runtime::tokio_runtime;

pub fn new_testing_oci_package_manager(
    base_path: PathBuf,
) -> OCIPackageManager<OCIArtifactDownloader, DirectoryManagerFs> {
    let runtime = tokio_runtime();

    let downloader = OCIArtifactDownloader::try_new(
        ProxyConfig::default(),
        runtime,
        ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        },
    )
    .unwrap();

    OCIPackageManager::new(downloader, DirectoryManagerFs, base_path)
}

/// Helpers ///
pub struct TestDataHelper;

impl TestDataHelper {
    fn create_data_to_compress(tmp_dir_to_compress: &Path, file_dir: &str, file_content: &str) {
        let file_path = tmp_dir_to_compress.join(file_dir);
        File::create(file_path.clone()).unwrap();
        std::fs::write(file_path.as_path(), file_content).unwrap();
    }

    pub fn compress_tar_gz(
        source_path: &Path,
        tmp_file_archive: &Path,
        content: &str,
        filename: &str,
    ) {
        Self::create_data_to_compress(source_path, filename, content);

        let tar_gz = File::create(tmp_file_archive).unwrap();
        let enc = GzEncoder::new(tar_gz, Compression::default());
        let mut tar = tar::Builder::new(enc);
        let file_path = source_path.join(filename);
        tar.append_path_with_name(&file_path, filename).unwrap();
        tar.finish().unwrap();
    }

    #[cfg(target_os = "windows")]
    pub fn compress_zip(
        source_path: &Path,
        tmp_file_archive: &Path,
        content: &str,
        filename: &str,
    ) {
        Self::create_data_to_compress(source_path, filename, content);

        let file = File::create(tmp_file_archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for entry in std::fs::read_dir(source_path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip.start_file(path.file_name().unwrap().to_string_lossy(), options)
                .unwrap();
            let mut f = File::open(&path).unwrap();
            std::io::copy(&mut f, &mut zip).unwrap();
        }

        zip.finish().unwrap();
    }

    pub fn test_tar_gz_uncompressed(tmp_dir_extracted: &Path, filename: &str) {
        assert!(tmp_dir_extracted.join(filename).exists());
    }
}
