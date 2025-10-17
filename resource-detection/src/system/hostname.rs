use super::detector::SystemDetectorError;

// TODO both implementation are similar to the one of the std library that is still unstable.
// Consider using it when it becomes stable: https://github.com/rust-lang/libs-team/issues/330

#[cfg(target_family = "unix")]
/// Get the system hostname for Unix-like systems.
pub fn get_hostname() -> Result<String, SystemDetectorError> {
    use nix::unistd::gethostname;
    gethostname()
        .map_err(|e| SystemDetectorError::HostnameError(e.to_string()))
        .map(|h| h.to_string_lossy().to_string())
}

#[cfg(target_family = "windows")]
/// Get the system hostname for Windows systems.
pub fn get_hostname() -> Result<String, SystemDetectorError> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    init();

    windows_link::link!("ws2_32.dll" "system" fn GetHostNameW(name : *mut u16, namelen : i32) -> i32);
    windows_link::link!("ws2_32.dll" "system" fn WSAGetLastError() -> i32);

    // The documentation of GetHostNameW says that a buffer size of 256 is
    // always enough.
    let mut buffer = [0; 256];

    // SAFETY: these parameters specify a valid, writable region of memory.
    unsafe {
        if GetHostNameW(buffer.as_mut_ptr(), buffer.len() as i32) != 0 {
            return Err(SystemDetectorError::HostnameError(format!(
                "GetHostNameW returned an error code: {}",
                WSAGetLastError()
            )));
        }
    }

    let len = buffer
        .iter()
        .position(|&w| w == 0)
        .ok_or(SystemDetectorError::HostnameError(
            "GetHostNameW did not return a null-terminated name".to_string(),
        ))?;

    Ok(OsString::from_wide(&buffer[..len])
        .to_string_lossy()
        .to_string())
}

#[cfg(target_family = "windows")]
/// Initialise the network stack for Windows.
fn init() {
    use std::sync::Once;

    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // We let the std library to call for us WSA startup
        let _ = std::net::UdpSocket::bind("127.0.0.1:0");
    });
}

#[cfg(test)]
mod tests {
    use super::get_hostname;

    #[test]
    fn test_get_hostname() {
        let hostname = get_hostname().expect("Failed to get hostname");
        assert!(!hostname.is_empty(), "Hostname should not be empty");
        println!("Hostname: {}", hostname);
    }
}
