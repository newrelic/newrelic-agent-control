use crate::agent_type::runtime_config::on_host::package::PackageType;
use flate2::read::GzDecoder;
use std::fs::File;
use std::path::Path;
use tar::Archive;
use thiserror::Error;
use tracing::{debug, instrument};
use zip::ZipArchive;

#[derive(Debug, Error)]
#[error("extract error: {0}")]
pub struct ExtractError(pub String);

impl PackageType {
    #[instrument(skip_all, fields(archive_path = %archive_path.to_string_lossy()),name = "extracting_archive")]
    pub fn extract(&self, archive_path: &Path, dest_path: &Path) -> Result<(), ExtractError> {
        match self {
            PackageType::Tar => extract_tar_gz(archive_path, dest_path),
            PackageType::Zip => extract_zip(archive_path, dest_path),
        }
    }
}

/// Extracts a tar.gz archive located at `archive_path` into the directory at `destination_path`.
/// This operation is relatively sensitive in that it will not write files outside of the path specified by dst.
/// Files in the archive which have a '..' in their path are skipped during the unpacking process.
fn extract_tar_gz(tar_path: &Path, destination_path: &Path) -> Result<(), ExtractError> {
    debug!("Extracting tar.gz archive to '{:?}'", destination_path);

    let tar_gz =
        File::open(tar_path).map_err(|e| ExtractError(format!("opening tar.gz file: {}", e)))?;
    let tar = GzDecoder::new(tar_gz);
    Archive::new(tar)
        .unpack(destination_path)
        .map_err(|e| ExtractError(format!("extracting tar.gz file: {e}",)))
}

/// Extracts a zip archive located at `zip_path` into the directory at `destination`.
/// Extraction is not atomic. If an error is encountered, some of the files may be left on disk.
/// Extract a Zip archive into a directory, overwriting files if they already exist. Paths are sanitized with ZipFile::enclosed_name.
fn extract_zip(zip_path: &Path, destination: &Path) -> Result<(), ExtractError> {
    debug!("Extracting zip archive to '{:?}'", destination);

    let file =
        File::open(zip_path).map_err(|e| ExtractError(format!("opening zip file: {}", e)))?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| ExtractError(format!("reading zip file: {}", e)))?;

    archive
        .extract(destination)
        .map_err(|e| ExtractError(format!("extracting zip file: {e}")))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::package::extract::PackageType::{Tar, Zip};
    use assert_matches::assert_matches;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::fs::File;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    #[test]
    fn test_extract_no_file() {
        let archive_path = Path::new("not-existing");
        let destination_path = Path::new("");

        let result = Tar.extract(archive_path, destination_path);
        assert_matches!(result, Err(ExtractError(e)) => {
            assert!(e.contains("opening tar.gz file"));
        });

        let result = Zip.extract(archive_path, destination_path);
        assert_matches!(result, Err(ExtractError(e)) => {
            assert!(e.contains("opening zip file"));
        });
    }

    #[test]
    fn test_extract_empty_and_wrong_format_file_tar() {
        let destination_path = Path::new("");
        let archive_dir = tempdir().unwrap();
        let archive_path = archive_dir.path().join("not_a_tar_gz_file.tar.gz");
        File::create(archive_path.clone()).unwrap();

        let result = Tar.extract(&archive_path, destination_path);
        assert_matches!(result, Err(ExtractError(e)) => {
            assert!(e.contains("extracting tar.gz file"));
        });

        std::fs::write(archive_path.clone(), "this is not a valid tar.gz content").unwrap();
        let result = Tar.extract(&archive_path, destination_path);
        assert_matches!(result, Err(ExtractError(e)) => {
            assert!(e.contains("extracting tar.gz file"));
        });
    }

    #[test]
    fn test_e2e_tar_gz() {
        let tmp_dir_to_compress = tempdir().unwrap();
        let tmp_dir_archive = tempdir().unwrap();
        let tmp_file_archive = tmp_dir_archive.path().join("my.tar.gz");

        create_data_to_compress(tmp_dir_to_compress.path());
        compress_tar_gz(tmp_dir_to_compress.path(), tmp_file_archive.as_path());

        let tmp_dir_extracted = tempdir().unwrap();
        let result = Tar.extract(&tmp_file_archive, tmp_dir_extracted.path());
        result.unwrap();

        assert!(tmp_dir_extracted.path().join("./file1.txt").exists());
        assert!(tmp_dir_extracted.path().join("./file2.txt").exists());
    }

    #[test]
    fn test_extract_empty_and_wrong_format_file_zip() {
        let destination_path = Path::new("test_output");
        let tmp_dir = tempdir().unwrap();
        let archive_path = tmp_dir.path().join("not_a_zip_file.zip");
        File::create(archive_path.clone()).unwrap();

        let result = Zip.extract(&archive_path, destination_path);
        assert_matches!(result, Err(ExtractError(e)) => {
            assert!(e.contains("reading zip file"));
        });

        std::fs::write(archive_path.clone(), "this is not a valid zip content").unwrap();
        let result = Zip.extract(&archive_path, destination_path);
        assert_matches!(result, Err(ExtractError(e)) => {
            assert!(e.contains("reading zip file"));
        });
    }

    #[test]
    fn test_e2e_zip() {
        let tmp_dir_to_compress = tempdir().unwrap();
        let tmp_dir_archive = tempdir().unwrap();
        let tmp_file_archive = tmp_dir_archive.path().join("my.zip");

        create_data_to_compress(tmp_dir_to_compress.path());
        compress_zip(tmp_dir_to_compress.path(), tmp_file_archive.as_path());

        let tmp_dir_extracted = tempdir().unwrap();
        let result = Zip.extract(&tmp_file_archive, tmp_dir_extracted.path());
        result.unwrap();

        assert!(tmp_dir_extracted.path().join("./file1.txt").exists());
        assert!(tmp_dir_extracted.path().join("./file2.txt").exists());
    }

    /// Helpers ///
    pub fn compress_zip(source_path: &Path, tmp_file_archive: &Path) {
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
}
