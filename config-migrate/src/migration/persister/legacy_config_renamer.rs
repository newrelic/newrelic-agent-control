use fs::{
    file_renamer::{FileRenamer, FileRenamerError},
    LocalFile,
};
use std::path::PathBuf;

const LEGACY_PATH_BCK_TOKEN: &str = "bck";

pub struct LegacyConfigRenamer<F: FileRenamer> {
    file_renamer: F,
}

impl Default for LegacyConfigRenamer<LocalFile> {
    fn default() -> Self {
        Self {
            file_renamer: LocalFile,
        }
    }
}

impl<F: FileRenamer> LegacyConfigRenamer<F> {
    pub fn rename_path(&self, path: &str) -> Result<(), FileRenamerError> {
        let original_path = PathBuf::from(path);

        let mut dest_path = PathBuf::from(path);
        let mut extension = LEGACY_PATH_BCK_TOKEN.to_string();
        if let Some(ext) = dest_path.extension() {
            extension = format!("{}.{}", ext.to_str().unwrap(), LEGACY_PATH_BCK_TOKEN);
        }
        dest_path.set_extension(extension);

        self.file_renamer.rename(&original_path, &dest_path)
    }
}

////////////////////////////////////////////////////////////////////////////////////
// TESTS
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod test {
    use fs::MockLocalFile;

    use super::*;

    #[test]
    fn test_rename_path_without_extension() {
        let mut file_renamer = MockLocalFile::new();

        let path = "no-extension";
        let expected_path = "no-extension.bck";
        let original_path = PathBuf::from(path);
        let dest_path = PathBuf::from(expected_path);

        file_renamer.should_rename(original_path.as_path(), dest_path.as_path());

        let legacy_config_renamer = LegacyConfigRenamer { file_renamer };

        assert!(legacy_config_renamer.rename_path(path).is_ok());
    }

    #[test]
    fn test_rename_path_with_extension() {
        let mut file_renamer = MockLocalFile::new();

        let path = "with-extension.d";
        let expected_path = "with-extension.d.bck";
        let original_path = PathBuf::from(path);
        let dest_path = PathBuf::from(expected_path);

        file_renamer.should_rename(original_path.as_path(), dest_path.as_path());

        let legacy_config_renamer = LegacyConfigRenamer { file_renamer };

        assert!(legacy_config_renamer.rename_path(path).is_ok());
    }
}
