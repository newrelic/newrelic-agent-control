use std::time::{SystemTime, SystemTimeError};

// TODO: we should abstract this so we can inject a mocked
// instance to tests and assert on structs that contain timestamps
pub fn get_sys_time_nano() -> Result<u64, SystemTimeError> {
    Ok(SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_nanos() as u64)
}
