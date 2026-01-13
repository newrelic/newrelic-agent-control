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
    const FILE1: &str = "file1.txt";
    const FILE2: &str = "file2.txt";
    const CONTENT: &str = "important content";

    fn create_data_to_compress(tmp_dir_to_compress: &Path) {
        let file_path_1 = tmp_dir_to_compress.join(TestDataHelper::FILE1);
        File::create(file_path_1.clone()).unwrap();
        let file_path_2 = tmp_dir_to_compress.join(TestDataHelper::FILE2);
        File::create(file_path_2.clone()).unwrap();

        std::fs::write(file_path_1.as_path(), TestDataHelper::CONTENT).unwrap();
        std::fs::write(file_path_2.as_path(), TestDataHelper::CONTENT).unwrap();
    }

    pub fn compress_tar_gz(source_path: &Path, tmp_file_archive: &Path) {
        TestDataHelper::create_data_to_compress(source_path);

        let tar_gz = File::create(tmp_file_archive).unwrap();
        let enc = GzEncoder::new(tar_gz, Compression::default());
        let mut tar = tar::Builder::new(enc);
        tar.append_dir_all(".", source_path).unwrap();
        tar.finish().unwrap();
    }

    pub fn test_data_uncompressed(tmp_dir_extracted: &Path) {
        assert!(tmp_dir_extracted.join(TestDataHelper::FILE1).exists());
        assert!(tmp_dir_extracted.join(TestDataHelper::FILE2).exists());
    }
}
