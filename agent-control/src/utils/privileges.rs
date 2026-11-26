//! Enabling of Windows token privileges for the current process.
//!
//! A privilege listed in a token is usually *disabled*, and Windows only honors it once it has
//! been enabled in that token. The state is process-wide and lasts for the life of the process.

use tracing::debug;
use windows::Win32::Foundation::{CloseHandle, ERROR_NOT_ALL_ASSIGNED, GetLastError, HANDLE, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{HSTRING, PCWSTR};

/// Privilege ("Create permanent shared objects") required to create permanent kernel objects,
/// such as the named Job Objects used to track sub-agents.
pub const SE_CREATE_PERMANENT_PRIVILEGE: &str = "SeCreatePermanentPrivilege";

/// Error returned when a privilege could not be enabled in the process token.
#[derive(Debug, thiserror::Error)]
pub enum PrivilegeError {
    #[error("looking up privilege {0}: {1}")]
    Lookup(String, #[source] windows::core::Error),
    #[error("opening process token: {0}")]
    OpenToken(windows::core::Error),
    #[error("adjusting privilege {0}: {1}")]
    Adjust(String, #[source] windows::core::Error),
    #[error(
        "privilege {0} is not held by the process token, grant it to the account running Agent Control"
    )]
    NotHeld(String),
}

/// Enables the privilege `name` (e.g. [SE_CREATE_PERMANENT_PRIVILEGE]) in the token of the
/// current process.
///
/// It fails if the token does not hold the privilege, which happens when the account was not
/// granted the corresponding user right. `NT AUTHORITY\SYSTEM` holds [SE_CREATE_PERMANENT_PRIVILEGE]
/// by default.
pub fn enable_privilege(name: &str) -> Result<(), PrivilegeError> {
    let mut luid = LUID::default();
    unsafe { LookupPrivilegeValueW(PCWSTR::null(), &HSTRING::from(name), &mut luid) }
        .map_err(|source| PrivilegeError::Lookup(name.to_string(), source))?;

    let mut token = HANDLE::default();
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
    }
    .map_err(PrivilegeError::OpenToken)?;

    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    let adjusted = unsafe { AdjustTokenPrivileges(token, false, Some(&privileges), 0, None, None) };

    // The call reports success even when the token does not hold the privilege; that case is
    // only signaled through the last error. It must be read before any other API call.
    let last_error = unsafe { GetLastError() };
    let _ = unsafe { CloseHandle(token) };

    adjusted.map_err(|source| PrivilegeError::Adjust(name.to_string(), source))?;
    if last_error == ERROR_NOT_ALL_ASSIGNED {
        return Err(PrivilegeError::NotHeld(name.to_string()));
    }

    debug!(privilege = name, "Privilege enabled");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use std::collections::HashMap;
    use windows::Win32::Security::{GetTokenInformation, LookupPrivilegeNameW, TokenPrivileges};
    use windows::core::PWSTR;

    #[test]
    fn unknown_privilege_fails() {
        let err = enable_privilege("SeDoesNotExistPrivilege").unwrap_err();
        assert_matches!(err, PrivilegeError::Lookup(_, _));
    }

    #[test]
    fn privilege_not_held_by_the_token_fails() {
        // Held only by `lsass`: neither `SYSTEM` nor administrator tokens have it.
        let err = enable_privilege("SeCreateTokenPrivilege").unwrap_err();
        assert_matches!(err, PrivilegeError::NotHeld(_));
    }

    // The privileges of the token are shared by the whole process. This is the only test that
    // changes them, so the requirements are asserted together to avoid synchronizing tests.
    #[test]
    fn enables_only_the_requested_privilege_and_idempotency() {
        let before = token_privileges();
        let target = disabled_privilege(&before);

        enable_privilege(&target).unwrap();

        let mut expected = before;
        expected.insert(target.clone(), true);
        assert_eq!(expected, token_privileges());

        enable_privilege(&target).unwrap();
        assert_eq!(expected, token_privileges());
    }

    /// Name of any privilege held by the token but currently disabled.
    fn disabled_privilege(privileges: &HashMap<String, bool>) -> String {
        privileges
            .iter()
            .find(|(_, enabled)| !**enabled)
            .map(|(name, _)| name.clone())
            .expect("the token holds no disabled privilege")
    }

    fn token_privileges() -> HashMap<String, bool> {
        let mut token = HANDLE::default();
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.unwrap();

        let mut len = 0;
        let _ = unsafe { GetTokenInformation(token, TokenPrivileges, None, 0, &mut len) };
        let mut buffer = vec![0u64; (len as usize).div_ceil(8)];
        unsafe {
            GetTokenInformation(
                token,
                TokenPrivileges,
                Some(buffer.as_mut_ptr().cast()),
                len,
                &mut len,
            )
        }
        .unwrap();
        let _ = unsafe { CloseHandle(token) };

        let privileges = unsafe { &*buffer.as_ptr().cast::<TOKEN_PRIVILEGES>() };
        let entries = unsafe {
            std::slice::from_raw_parts(
                privileges.Privileges.as_ptr(),
                privileges.PrivilegeCount as usize,
            )
        };
        entries
            .iter()
            .map(|entry| {
                (
                    privilege_name(&entry.Luid),
                    entry.Attributes.0 & SE_PRIVILEGE_ENABLED.0 != 0,
                )
            })
            .collect()
    }

    fn privilege_name(luid: &LUID) -> String {
        let mut name = [0u16; 128];
        let mut len = name.len() as u32;
        unsafe {
            LookupPrivilegeNameW(
                PCWSTR::null(),
                luid,
                Some(PWSTR(name.as_mut_ptr())),
                &mut len,
            )
        }
        .unwrap();
        String::from_utf16_lossy(&name[..len as usize])
    }
}
