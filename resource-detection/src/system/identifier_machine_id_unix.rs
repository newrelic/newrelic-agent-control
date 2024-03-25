use std::path::{Path, PathBuf};

use fs::{
    file_reader::{FileReader, FileReaderError},
    LocalFile,
};

use super::detector::SystemDetectorError;

const MACHINE_ID_PATH: &str =
    konst::option::unwrap_or!(option_env!("TEST_MACHINE_ID_PATH"), "/etc/machine-id");

const DBUS_MACHINE_ID_PATH: &str = konst::option::unwrap_or!(
    option_env!("TEST_DBUS_MACHINE_ID_PATH"),
    "/var/lib/dbus/machine-id"
);

pub(super) struct IdentifierProviderMachineId<F> {
    machine_id_path: PathBuf,
    dbus_machine_id_path: PathBuf,
    file_reader: F,
}

impl<F> IdentifierProviderMachineId<F>
where
    F: FileReader,
{
    fn read_content(&self, file_path: &Path) -> Result<String, FileReaderError> {
        self.file_reader.read(file_path)
    }

    pub(super) fn provide(&self) -> Result<String, SystemDetectorError> {
        self.read_content(self.machine_id_path.as_path())
            .or_else(|_| self.read_content(self.dbus_machine_id_path.as_path()))
            .map(|s: String| s.trim().to_string())
            .map_err(|e| SystemDetectorError::MachineIDError(e.to_string()))
    }
}

impl Default for IdentifierProviderMachineId<LocalFile> {
    fn default() -> Self {
        Self {
            machine_id_path: PathBuf::from(MACHINE_ID_PATH),
            dbus_machine_id_path: PathBuf::from(DBUS_MACHINE_ID_PATH),
            file_reader: LocalFile,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fs::mock::MockLocalFile;
    use std::path::Path;

    impl<F> IdentifierProviderMachineId<F>
    where
        F: FileReader,
    {
        fn new(machine_id_path: &Path, dbus_path: &Path, file_reader: F) -> Self {
            Self {
                file_reader,
                machine_id_path: PathBuf::from(machine_id_path),
                dbus_machine_id_path: PathBuf::from(dbus_path),
            }
        }
    }

    #[test]
    fn test_machine_id_is_retrieved() {
        let mut file_reader = MockLocalFile::default();

        let path = PathBuf::from("/some/path");
        let expected_machine_id = String::from("some machine id");

        file_reader.should_read(path.as_path(), expected_machine_id.clone());

        let provider =
            IdentifierProviderMachineId::new(path.as_path(), path.as_path(), file_reader);

        let machine_id = provider.provide().unwrap();
        assert_eq!(expected_machine_id, machine_id);
    }

    #[test]
    fn test_dbus_machine_id_is_retrieved() {
        let mut file_reader = MockLocalFile::default();

        let machine_id_path = PathBuf::from("/some/path");
        let dbus_machine_id_path = PathBuf::from("/some/path_2");
        let expected_machine_id = String::from("some machine id");

        file_reader.should_not_read_file_not_found(
            machine_id_path.as_path(),
            String::from("some error message"),
        );
        file_reader.should_read(dbus_machine_id_path.as_path(), expected_machine_id.clone());

        let provider = IdentifierProviderMachineId::new(
            machine_id_path.as_path(),
            dbus_machine_id_path.as_path(),
            file_reader,
        );

        let machine_id = provider.provide().unwrap();
        assert_eq!(expected_machine_id, machine_id);
    }

    #[test]
    fn test_error_retrieving_machine_id() {
        let mut file_reader = MockLocalFile::default();

        let path = PathBuf::from("/some/path");

        file_reader
            .should_not_read_file_not_found(path.as_path(), String::from("some error message"));
        file_reader
            .should_not_read_file_not_found(path.as_path(), String::from("some error message"));

        let provider =
            IdentifierProviderMachineId::new(path.as_path(), path.as_path(), file_reader);

        let result = provider.provide();
        assert!(result.is_err());
        assert_eq!(
            String::from("error getting machine-id: `file not found: `some error message``"),
            result.unwrap_err().to_string()
        );
    }
}
