use crate::common::runtime::tokio_runtime;
use flate2::Compression;
use flate2::write::GzEncoder;
use fs::directory_manager::DirectoryManagerFs;
use newrelic_agent_control::{
    http::config::ProxyConfig,
    oci,
    package::oci::{downloader::OCIArtifactDownloader, package_manager::OCIPackageManager},
};
use oci_client::client::{ClientConfig, ClientProtocol};
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

pub fn new_testing_oci_package_manager(
    base_path: PathBuf,
    registry: String,
) -> OCIPackageManager<OCIArtifactDownloader, DirectoryManagerFs> {
    let client = oci::Client::try_new(
        ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        },
        ProxyConfig::default(),
        tokio_runtime(),
    )
    .unwrap();
    let downloader = OCIArtifactDownloader::new(client, registry, Default::default(), false);

    OCIPackageManager::new(downloader, DirectoryManagerFs, base_path)
}

/// Helpers ///
pub struct TestDataHelper;

impl TestDataHelper {
    pub fn compress_tar_gz(
        source_path: &Path,
        tmp_file_archive: &Path,
        content: &str,
        filename: &str,
    ) {
        let file_path = Self::create_data_to_compress(source_path, filename, content);

        let tar_gz = File::create(tmp_file_archive).unwrap();
        let enc = GzEncoder::new(tar_gz, Compression::default());
        let mut tar = tar::Builder::new(enc);
        tar.append_path_with_name(file_path, filename).unwrap();
        tar.finish().unwrap();
    }

    /// Compresses a single file into a tar.gz archive, storing it with mode 0o755.
    /// The entry name in the archive is taken from the file's own name.
    #[cfg(target_family = "unix")]
    pub fn compress_tar_gz_executable(file_path: &Path, archive_path: &Path) {
        let filename = file_path.file_name().expect("file_path has no filename");
        let tar_gz = File::create(archive_path).unwrap();
        let enc = GzEncoder::new(tar_gz, Compression::default());
        let mut tar = tar::Builder::new(enc);
        let mut file = File::open(file_path).unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(file_path.metadata().unwrap().len());
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, filename, &mut file).unwrap();
        tar.finish().unwrap();
    }

    #[cfg(target_os = "windows")]
    pub fn compress_zip(
        source_path: &Path,
        tmp_file_archive: &Path,
        content: &str,
        filename: &str,
    ) {
        let file_path = Self::create_data_to_compress(source_path, filename, content);
        Self::compress_zip_file(&file_path, tmp_file_archive);
    }

    #[cfg(target_os = "windows")]
    pub fn compress_zip_file(file_path: &Path, archive_path: &Path) {
        use zip::write::SimpleFileOptions;
        let filename = file_path.file_name().expect("file_path has no filename");
        let file = File::create(archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(filename.to_string_lossy(), options).unwrap();
        let mut f = File::open(file_path).unwrap();
        std::io::copy(&mut f, &mut zip).unwrap();
        zip.finish().unwrap();
    }

    pub fn test_tar_gz_uncompressed(tmp_dir_extracted: &Path, filename: &str) {
        assert!(tmp_dir_extracted.join(filename).exists());
    }

    fn create_data_to_compress(
        tmp_dir_to_compress: &Path,
        file_dir: &str,
        file_content: &str,
    ) -> PathBuf {
        let file_path = tmp_dir_to_compress.join(file_dir);
        File::create(&file_path).unwrap();
        std::fs::write(file_path.as_path(), file_content).unwrap();
        file_path
    }
}
