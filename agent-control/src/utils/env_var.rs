use fs::file_reader::FileReader;
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
// Key validation regex, start with letter/underscore, then letters/digits/underscore.
const KEY_VALIDATION_REGEX: &str = r"^[A-Za-z_][A-Za-z0-9_]*$";

/// Load environment variables into the current process from a YAML file located at `path`.
/// The YAML file should contain key-value pairs where keys are the environment variable names
/// and values are their corresponding values.
/// # Errors
/// Returns an error if the file cannot be read, if the YAML is malformed,
/// or if any key doesn't match `^[A-Za-z_][A-Za-z0-9_]*$` or if any value contains NUL characters.
/// # Safety
/// This function uses `std::env::set_var` which is not thread-safe on all platforms.
/// It is the caller's responsibility to ensure that no other threads are concurrently
/// accessing environment variables while this function is executing.
pub fn load_env_yaml_file(path: &Path) -> Result<(), Box<dyn Error>> {
    let file = fs::LocalFile {};
    let content = file.read(path)?;

    let env_vars: HashMap<String, String> = serde_yaml::from_str(&content)?;

    load_env_from_hashmap(env_vars)
}

fn load_env_from_hashmap(env_vars: HashMap<String, String>) -> Result<(), Box<dyn Error>> {
    let re = regex::Regex::new(KEY_VALIDATION_REGEX).unwrap();

    for (key, value) in env_vars {
        // As per docs for std::env::set_var:
        // "This function may panic if key is empty, contains an ASCII equals sign '=' or the NUL character '\0',
        // or when value contains the NUL character."
        // We validate both key and value to avoid panics.
        if !re.is_match(&key) {
            return Err(format!("invalid key, must match {KEY_VALIDATION_REGEX}: {key}").into());
        }
        if value.contains('\0') {
            return Err(format!("invalid value (contains NUL) for key: {key}").into());
        }

        unsafe {
            std::env::set_var(key, value);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serial_test::serial;

    #[test]
    #[serial]
    fn load_env_from_hashmap_success() {
        let mut env_vars = HashMap::new();
        env_vars.insert("some_key".to_string(), "some_value".to_string());
        assert!(load_env_from_hashmap(env_vars).is_ok());
        assert!(std::env::var("some_key").unwrap() == "some_value");
        unsafe {
            std::env::remove_var("some_key");
        }
    }

    #[rstest]
    #[case::empty("")]
    #[case::starts_with_digit("9ABC")]
    #[case::contains_dash("ABC-DEF")]
    #[case::contains_space("ABC DEF")]
    #[case::contains_dot("A.B")]
    #[case::starts_with_dot(".ABC")]
    #[case::contains_dollar("A$B")]
    #[case::contains_emoji("A😊B")]
    #[case::contains_slash("ABC/DEF")]
    #[case::contains_plus("ABC+DEF")]
    #[case::contains_nul("NULL\0CHAR")]
    fn invalid_keys_error(#[case] invalid_key: &str) {
        let mut env_vars = HashMap::new();
        env_vars.insert(invalid_key.to_string(), "some_value".to_string());

        let msg = load_env_from_hashmap(env_vars).err().unwrap().to_string();
        assert!(
            msg.contains("invalid key"),
            "unexpected error message: {}",
            msg
        );
    }

    #[test]
    fn values_must_not_contain_nul() {
        let mut env_vars = HashMap::new();
        env_vars.insert("VALID_KEY".to_string(), "value_with_nul\0".to_string());

        let msg = load_env_from_hashmap(env_vars).err().unwrap().to_string();
        assert!(
            msg.contains("invalid value"),
            "unexpected error message: {}",
            msg
        );
    }
}
