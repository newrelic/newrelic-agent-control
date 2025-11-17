#[cfg(target_family = "windows")]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(target_family = "windows")]
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
#[cfg(target_family = "windows")]
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct IsElevatedError(String);

pub fn is_elevated() -> Result<bool, IsElevatedError> {
    #[cfg(target_family = "unix")]
    return Ok(nix::unistd::Uid::effective().is_root());

    #[cfg(target_family = "windows")]
    is_elevated_windows()
}

#[cfg(target_family = "windows")]
fn is_elevated_windows() -> Result<bool, IsElevatedError> {
    unsafe {
        let mut token_handle: HANDLE = std::ptr::null_mut();
        let process = GetCurrentProcess();

        // https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocesstoken
        // An access token contains the security information for a logon session and every process
        // executed on behalf of the user has a copy of the token. Here we get that token.
        if OpenProcessToken(process, TOKEN_QUERY, &mut token_handle) == 0 {
            return Err(IsElevatedError("Failed to open process token.".to_string()));
        }

        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut return_length = 0;

        // https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-gettokeninformation
        // GetTokenInformation requires a pointer to a buffer (elevation) the function fills with the requested information.
        // The second parameter is the class (TokenElevation) we want to get information from the Token.
        let result = GetTokenInformation(
            token_handle,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut return_length,
        );

        CloseHandle(token_handle);

        if result == 0 {
            return Err(IsElevatedError(
                "Failed to get token information to check user rights.".to_string(),
            ));
        }

        Ok(elevation.TokenIsElevated != 0)
    }
}
