#![cfg(target_family = "windows")]
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, GetLastError};
use windows_sys::Win32::Security::Authorization::{
    EXPLICIT_ACCESS_W, SE_FILE_OBJECT, SET_ACCESS, SetEntriesInAclW, SetNamedSecurityInfoW,
    TRUSTEE_IS_SID, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    ACL, CreateWellKnownSid, DACL_SECURITY_INFORMATION, NO_INHERITANCE,
    PROTECTED_DACL_SECURITY_INFORMATION, SECURITY_MAX_SID_SIZE, WinBuiltinAdministratorsSid,
};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct PermissionError(String);

fn get_administrator_sid() -> Result<Vec<u8>, PermissionError> {
    let mut sid_size = SECURITY_MAX_SID_SIZE as u32;
    let mut sid: Vec<u8> = vec![0; sid_size as usize];

    unsafe {
        // We define the buffer with the right size to be retrieved avoiding an error 122 (ERROR_INSUFICIENT_BUFFER)

        if CreateWellKnownSid(
            WinBuiltinAdministratorsSid,
            ptr::null_mut(),
            sid.as_mut_ptr() as *mut _,
            &mut sid_size,
        ) == 0
        {
            return Err(PermissionError(format!(
                "Failed to create administrator SID. Error: {}",
                GetLastError()
            )));
        }
    }

    Ok(sid)
}

/// set_file_permissions_for_administrator removes any other ACL from a file only granting
/// read and write to Administrators.
pub fn set_file_permissions_for_administrator(path: &Path) -> Result<(), PermissionError> {
    // Conversion to UTF-16 format (native string representation in Windows OS)
    let path_wstr: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();

    let admin_sid = get_administrator_sid()
        .map_err(|e| PermissionError(format!("Failed to get administrator sid: {}", e)))?;

    // Define the trustee (windows ACL entity) with the current user SID
    let trustee = TRUSTEE_W {
        TrusteeForm: TRUSTEE_IS_SID,
        ptstrName: admin_sid.as_ptr() as *mut _,
        ..Default::default()
    };

    // Define the access entry to allow read and write for the trustee
    let access_entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_READ | GENERIC_WRITE,
        grfAccessMode: SET_ACCESS,
        grfInheritance: NO_INHERITANCE,
        Trustee: trustee,
    };

    // Create a new ACL with the access entry
    let mut acl: *mut ACL = ptr::null_mut();
    unsafe {
        // https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setentriesinaclw
        // creates a new ACL by merging new ACL into the AC provided in the 3rd parameter
        // we set it to null because we overwrite the old ACL
        let result = SetEntriesInAclW(1, &access_entry, ptr::null_mut(), &mut acl);

        if result != 0 {
            return Err(PermissionError("Failed to set entries in ACL".to_string()));
        }

        // https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setnamedsecurityinfow
        // Set the security descriptor with the new ACL.
        // PROTECTED_DACL_SECURITY_INFORMATION is removing inheritance so the ACL will only
        // apply to administrators.
        let result = SetNamedSecurityInfoW(
            path_wstr.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            acl,
            ptr::null_mut(),
        );

        if result != 0 {
            return Err(PermissionError(
                "Failed to set security descriptor".to_string(),
            ));
        }

        Ok(())
    }
}
