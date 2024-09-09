use std::time::{Duration, SystemTime, SystemTimeError};

// TODO: we should abstract this so we can inject a mocked
// instance to tests and assert on structs that contain timestamps
pub fn get_sys_time_nano() -> Result<u64, SystemTimeError> {
    Ok(SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_nanos() as u64)
}

/// Converts a unix epoch timestamp in nanoseconds to a `SystemTime`.
pub fn sys_time_from_unix_timestamp(nanos: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_nanos(nanos)
}
/// Converts a unix epoch timestamp in nanoseconds to a `SystemTime`.
pub fn unix_timestamp_from_sys_time(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
