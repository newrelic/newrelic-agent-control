use std::path::PathBuf;

#[cfg_attr(test, mockall_double::double)]
use crate::file_reader::FSFileReader;

use super::detector::SystemDetectorError;

const MACHINE_ID_PATH: &str = "/etc/machine-id";

pub(super) struct IdentifierProviderMachineId {
    machine_id_path: PathBuf,
    file_reader: FSFileReader,
}

#[cfg_attr(test, mockall::automock)]
impl IdentifierProviderMachineId {
    pub(super) fn provide(&self) -> Result<String, SystemDetectorError> {
        self.file_reader
            .read(self.machine_id_path.as_path())
            .map(|s: String| s.trim().to_string())
            .map_err(|e| SystemDetectorError::MachineIDError(e.to_string()))
    }
}

impl Default for IdentifierProviderMachineId {
    fn default() -> Self {
        Self {
            machine_id_path: PathBuf::from(MACHINE_ID_PATH),
            file_reader: FSFileReader::default(),
        }
    }
}

#[cfg(test)]
mod test {

    use crate::file_reader::MockFSFileReader;

    use super::*;
    use std::path::Path;

    impl IdentifierProviderMachineId {
        fn new(some_path: &Path, file_reader: MockFSFileReader) -> Self {
            Self {
                file_reader,
                machine_id_path: PathBuf::from(some_path),
            }
        }
    }

    impl MockIdentifierProviderMachineId {
        pub fn should_provide(&mut self, machine_id: String) {
            self.expect_provide()
                .returning(move || Ok(machine_id.clone()));
        }

        pub fn should_not_provide(&mut self, err: SystemDetectorError) {
            self.expect_provide().returning(move || Err(err.clone()));
        }
    }

    #[test]
    fn test_machine_id_is_retrieved() {
        let mut file_reader = MockFSFileReader::default();

        let path = PathBuf::from("/some/path");
        let expected_machine_id = String::from("some machine id");

        file_reader.should_read(path.as_path(), expected_machine_id.clone());

        let provider = IdentifierProviderMachineId::new(path.as_path(), file_reader);

        let machine_id = provider.provide().unwrap();
        assert_eq!(expected_machine_id, machine_id);
    }

    #[test]
    fn test_error_retrieving_machine_id() {
        let mut file_reader = MockFSFileReader::default();

        let path = PathBuf::from("/some/path");

        file_reader
            .should_not_read_file_not_found(path.as_path(), String::from("some error message"));

        let provider = IdentifierProviderMachineId::new(path.as_path(), file_reader);

        let result = provider.provide();
        assert!(result.is_err());
        assert_eq!(
            String::from("error getting machine-id: `file not found: `some error message``"),
            result.unwrap_err().to_string()
        );
    }
}
