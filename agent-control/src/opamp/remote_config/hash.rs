use serde::{Deserialize, Serialize};

use super::report::OpampRemoteConfigStatus;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Hash, Eq)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "state")]
enum ConfigState {
    Applying,
    Applied,
    Failed { error_message: String },
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Hash, Eq)]
pub struct Hash {
    hash: String,
    #[serde(flatten)]
    state: ConfigState,
}

impl Hash {
    pub fn new(hash: String) -> Self {
        Self {
            hash,
            state: ConfigState::Applying,
        }
    }

    pub fn apply(&mut self) {
        self.state = ConfigState::Applied;
    }

    // It is mandatory for a failed hash to have the error
    pub fn fail(&mut self, error_message: String) {
        self.state = ConfigState::Failed { error_message };
    }

    pub fn get(&self) -> String {
        self.hash.clone()
    }

    pub fn is_applied(&self) -> bool {
        self.state == ConfigState::Applied
    }

    pub fn is_applying(&self) -> bool {
        self.state == ConfigState::Applying
    }

    pub fn is_failed(&self) -> bool {
        // if let self.state = ConfigState::Failed(msg)
        matches!(&self.state, ConfigState::Failed { .. })
    }

    pub fn error_message(&self) -> Option<String> {
        match &self.state {
            ConfigState::Failed { error_message: msg } => Some(msg.clone()),
            _ => None,
        }
    }
}

impl From<&Hash> for OpampRemoteConfigStatus {
    fn from(hash: &Hash) -> Self {
        match &hash.state {
            ConfigState::Applying => Self::Applying,
            ConfigState::Applied => Self::Applied,
            ConfigState::Failed { error_message } => Self::Error(error_message.to_owned()),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////
// TESTS
////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod tests {
    use super::{ConfigState, Hash};

    impl Hash {
        pub fn applied(hash: String) -> Self {
            Self {
                hash,
                state: ConfigState::Applied,
            }
        }

        pub fn failed(hash: String, error_message: String) -> Self {
            Self {
                hash,
                state: ConfigState::Failed { error_message },
            }
        }
    }

    #[test]
    fn test_config_state_default_status() {
        //default status for a hash should be applying
        let hash = Hash::new("some-hash".into());
        assert!(hash.is_applying())
    }

    #[test]
    fn test_config_state_transition() {
        // hash can change state. This is not ideal, as an applied hash should not go to failed
        let mut hash = Hash::new("some-hash".into());
        assert!(hash.is_applying());
        hash.apply();
        assert!(hash.is_applied());
        hash.fail("this is an error message".to_string());
        assert!(hash.is_failed());
    }

    #[test]
    fn test_hash_serialization() {
        let mut hash = Hash::new("123456789".to_string());
        let expected = "hash: '123456789'\nstate: applying\n";
        assert_eq!(expected, serde_yaml::to_string(&hash).unwrap());

        hash.apply();
        let expected = "hash: '123456789'\nstate: applied\n";
        assert_eq!(expected, serde_yaml::to_string(&hash).unwrap());

        hash.fail("this is an error message".to_string());
        let expected =
            "hash: '123456789'\nstate: failed\nerror_message: this is an error message\n";
        assert_eq!(expected, serde_yaml::to_string(&hash).unwrap());
    }

    #[test]
    fn test_hash_deserialization() {
        let mut hash = Hash::new("123456789".to_string());
        let content = "hash: '123456789'\nstate: applying\n";
        assert_eq!(hash, serde_yaml::from_str::<Hash>(content).unwrap());

        hash.apply();
        let content = "hash: '123456789'\nstate: applied\n";
        assert_eq!(hash, serde_yaml::from_str::<Hash>(content).unwrap());

        hash.fail("this is an error message".to_string());
        let content = "hash: '123456789'\nstate: failed\nerror_message: this is an error message\n";
        assert_eq!(hash, serde_yaml::from_str::<Hash>(content).unwrap());
    }
}
