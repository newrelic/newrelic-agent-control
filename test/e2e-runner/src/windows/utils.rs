use std::env;
use std::path::PathBuf;

pub fn as_user_dir(path: impl AsRef<str>) -> String {
    let user_profile = env::var("USERPROFILE").unwrap();
    PathBuf::from(format!("{}{}", user_profile, path.as_ref()))
        .to_string_lossy()
        .to_string()
}
