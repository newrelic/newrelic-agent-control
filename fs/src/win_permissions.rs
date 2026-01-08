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
    let mut sid_size = SECURITY_MAX_SID_SIZE;
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

#[cfg(test)]
pub mod tests {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        Security::{
            ACCESS_ALLOWED_ACE, ACL_SIZE_INFORMATION, AclSizeInformation,
            Authorization::GetNamedSecurityInfoW, EqualSid, GetAce, GetAclInformation,
        },
        Storage::FileSystem::{FILE_GENERIC_READ, FILE_GENERIC_WRITE},
    };

    use super::*;

    /// Asserts directory permissions are set to only allow Administrators read and write access on Windows.
    ///
    /// This is a helper function that checks the following:
    /// 1. The DACL of the file contains exactly one ACE.
    /// 2. The ACE is for the Administrators SID.
    /// 3. The ACE grants FILE_GENERIC_READ and FILE_GENERIC_WRITE permissions.
    pub fn assert_windows_permissions(path: &Path) {
        let mut admin_sid_size = SECURITY_MAX_SID_SIZE;
        let mut admin_sid: Vec<u8> = vec![0; admin_sid_size as usize];

        unsafe {
            // Get Administrator SID
            let sid_result = CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                ptr::null_mut(),
                admin_sid.as_mut_ptr() as *mut _,
                &mut admin_sid_size,
            );
            assert_ne!(sid_result, 0, "Failed to create administrator SID");

            // Get file's DACL
            let path_wstr: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let mut dacl: *mut ACL = ptr::null_mut();
            let mut security_descriptor: *mut std::ffi::c_void = ptr::null_mut();

            let result = GetNamedSecurityInfoW(
                path_wstr.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut security_descriptor,
            );

            assert_eq!(result, ERROR_SUCCESS, "Failed to get security info");
            assert!(!dacl.is_null(), "DACL should not be null");

            // Verify exactly 1 ACE
            let mut acl_size_info: ACL_SIZE_INFORMATION = std::mem::zeroed();
            let info_result = GetAclInformation(
                dacl,
                &mut acl_size_info as *mut _ as *mut _,
                std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            );

            assert_ne!(info_result, 0, "Failed to get ACL information");
            assert_eq!(
                acl_size_info.AceCount, 1,
                "Should have exactly 1 ACE (Administrators only)"
            );

            // Verify ACE is for Administrators with Read/Write permissions
            let mut ace_ptr: *mut std::ffi::c_void = ptr::null_mut();
            let ace_result = GetAce(dacl, 0, &mut ace_ptr);
            assert_ne!(ace_result, 0, "Failed to get ACE");

            let ace = &*(ace_ptr as *const ACCESS_ALLOWED_ACE);
            let sid_in_ace = &ace.SidStart as *const u32 as *mut std::ffi::c_void;
            let sids_equal = EqualSid(sid_in_ace, admin_sid.as_mut_ptr() as *mut _);
            assert_ne!(sids_equal, 0, "ACE SID should match Administrators SID");

            // FILE_GENERIC_READ and FILE_GENERIC_WRITE are what GENERIC_READ/WRITE map to
            let expected_mask = FILE_GENERIC_READ | FILE_GENERIC_WRITE;
            assert_eq!(
                ace.Mask, expected_mask,
                "ACE should have FILE_GENERIC_READ and FILE_GENERIC_WRITE permissions"
            );
        }
    }
}
