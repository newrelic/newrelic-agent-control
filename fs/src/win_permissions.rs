use std::ffi;
use std::io;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use tracing::trace;
use windows::Win32::Security::Authorization::{
    EXPLICIT_ACCESS_W, GetNamedSecurityInfoW, SE_FILE_OBJECT, SET_ACCESS, SetEntriesInAclW,
    SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_W,
};
use windows::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION, AclSizeInformation, CONTAINER_INHERIT_ACE,
    CreateWellKnownSid, DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation,
    OBJECT_INHERIT_ACE, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_MAX_SID_SIZE, WinBuiltinAdministratorsSid,
};
use windows::Win32::Storage::FileSystem::{
    DELETE, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
};
use windows::core::{PCWSTR, PWSTR};

/// Error returned when setting Windows file permissions (ACLs) fails.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct PermissionError(String);

fn get_administrator_sid() -> Result<Vec<u8>, PermissionError> {
    let mut sid_size = SECURITY_MAX_SID_SIZE;
    let mut sid: Vec<u8> = vec![0; sid_size as usize];

    unsafe {
        // We define the buffer with the right size to be retrieved avoiding an error 122 (ERROR_INSUFICIENT_BUFFER)
        CreateWellKnownSid(
            WinBuiltinAdministratorsSid,
            None,
            Some(PSID(sid.as_mut_ptr() as *mut ffi::c_void)),
            &mut sid_size,
        )
        .map_err(|e| PermissionError(format!("Failed to create administrator SID: {e}")))?;
    }

    Ok(sid)
}

/// Removes any other ACL from a file only granting
/// read, write, execute, and delete to Administrators. DELETE is needed so the same Administrator
/// that wrote the file can later remove it during filesystem reconciliation. Execute is needed so
/// executables placed under a managed directory (e.g. sub-agent assets) can be spawned and so
/// managed directories stay traversable.
///
/// The Administrators ACE is made inheritable (`OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE`) so
/// that files and subdirectories a sub-agent creates at runtime inside an AC-managed directory
/// (e.g. `newrelic-infra.log` under `NRIA_APP_DATA_DIR`) inherit this access. Without the inherit
/// flags, and with the DACL protected from parent inheritance below, such runtime-created files
/// would be created with an empty DACL and be inaccessible even to the Administrator/LocalSystem
/// process that created them ("Access is denied"). Inheritance keeps everything Administrators-only.
pub fn set_file_permissions_for_administrator(path: &Path) -> Result<(), PermissionError> {
    // Conversion to UTF-16 format (native string representation in Windows OS)
    let path_wstr: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();

    let admin_sid = get_administrator_sid()
        .map_err(|e| PermissionError(format!("Failed to get administrator sid: {}", e)))?;

    // Define the trustee (windows ACL entity) with the current user SID
    let trustee = TRUSTEE_W {
        TrusteeForm: TRUSTEE_IS_SID,
        ptstrName: PWSTR(admin_sid.as_ptr() as *mut u16),
        ..Default::default()
    };

    // Define the access entry to allow read, write, and delete for the trustee.
    // The ACE is inheritable (`OBJECT_INHERIT_ACE` | `CONTAINER_INHERIT_ACE`) so child files and
    // subdirectories created later at runtime inherit Administrators access. On a leaf file these
    // inheritance flags are a no-op (Windows strips them); on a directory they are what lets a
    // sub-agent open and rotate the log files it creates inside AC-managed directories.
    //
    // Rights are the already-mapped specific rights (`FILE_GENERIC_*`), not the `GENERIC_*` aliases.
    //
    // `FILE_GENERIC_EXECUTE` is included so that executables Agent Control places inside a managed
    // directory (notably the new binary the self-updater downloads and spawns for its dry-run verify)
    // inherit execute rights — and so the directory itself is traversable.
    //
    // Everything stays Administrators-only, so this is not a privilege widening.
    let access_entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: (FILE_GENERIC_READ
            | FILE_GENERIC_WRITE
            | FILE_GENERIC_EXECUTE
            | DELETE)
            .0,
        grfAccessMode: SET_ACCESS,
        grfInheritance: OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
        Trustee: trustee,
    };

    // Create a new ACL with the access entry
    let mut acl: *mut ACL = ptr::null_mut();
    unsafe {
        // https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setentriesinaclw
        // creates a new ACL by merging new ACL into the AC provided in the 2nd parameter.
        // We pass None because we overwrite the old ACL
        SetEntriesInAclW(Some(&[access_entry]), None, &mut acl)
            .ok()
            .map_err(|e| PermissionError(format!("Failed to set entries in ACL: {e}")))?;

        // https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setnamedsecurityinfow
        // Set the security descriptor with the new ACL.
        // PROTECTED_DACL_SECURITY_INFORMATION is removing inheritance so the ACL will only
        // apply to administrators.
        SetNamedSecurityInfoW(
            PCWSTR(path_wstr.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(acl as *const ACL),
            None,
        )
        .ok()
        .map_err(|e| PermissionError(format!("Failed to set security descriptor: {e}")))?;

        trace!(path = %path.display(), "applied Administrators-only DACL");
        Ok(())
    }
}

/// True iff `ace` is an `Allow` ACE for `admin_sid` granting the full managed rights
/// (`FILE_GENERIC_READ | WRITE | EXECUTE | DELETE`, i.e. Modify) and, when `require_inheritable`
/// (the entry is a directory), inheritable (`OI|CI`). This is the policy that
/// `permissions_need_repair` scans a DACL for; the only FFI it performs is the SID comparison.
fn grants_managed_admin_access(
    ace: &ACCESS_ALLOWED_ACE,
    admin_sid: &[u8],
    require_inheritable: bool,
) -> bool {
    let expected_mask = (FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_GENERIC_EXECUTE | DELETE).0;
    let required_inherit = (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE).0 as u8;

    // ACCESS_ALLOWED_ACE_TYPE == 0; only `Allow` ACEs grant access.
    let is_allow = ace.Header.AceType == 0;
    // EqualSid returns Ok only when the SIDs match; Err is the normal "different principal" signal,
    // so a non-Administrators ACE simply fails this conjunct rather than being an error.
    let is_admin = unsafe {
        EqualSid(
            PSID(&ace.SidStart as *const u32 as *mut ffi::c_void),
            PSID(admin_sid.as_ptr() as *mut ffi::c_void),
        )
    }
    .is_ok();
    let has_rights = ace.Mask & expected_mask == expected_mask;
    let inheritable =
        !require_inheritable || ace.Header.AceFlags & required_inherit == required_inherit;

    is_allow && is_admin && has_rights && inheritable
}

/// Returns whether `path`'s DACL must be re-stamped by the recursive repair.
///
/// It needs repair unless it contains an `Allow` ACE for `BUILTIN\Administrators` whose mask includes
/// the full managed rights (`FILE_GENERIC_READ | WRITE | EXECUTE | DELETE`, i.e. Modify) and — for a
/// directory — is inheritable (`OI|CI`). This catches every state:
///   - an **empty** DACL (zero ACEs, denies everyone incl. SYSTEM);
///   - a **NULL** DACL (grants everyone) or an unreadable one;
///   - a **populated but insufficient** DACL, e.g. the old `Administrators:(R,W)` (mask `0x12019f`,
///     no `DELETE`) that leaves stored remote configs undeletable on decommission, or a
///     non-inheritable directory ACE (NR-601065).
///
/// A conforming entry returns `false`, so a healthy tree is not rewritten on every startup. Only the
/// entries that actually lack the managed access are repaired.
///
/// Caveat: this looks for *any* matching Administrators `Allow` ACE with a sufficient mask, without
/// regard to ACE order. It does not check whether an earlier `Deny` ACE in the same DACL would negate
/// that `Allow` at Windows' actual access-check time — so a DACL with e.g. `[Deny Administrators:
/// Delete][Allow Administrators: Modify]` would be reported as conforming even though delete is
/// effectively denied. Not currently reachable: every DACL this code writes fully replaces the
/// existing one (`SetEntriesInAclW` is called with no old ACL to merge), so
/// `set_file_permissions_for_administrator` can never itself leave behind a stray `Deny` ACE for this
/// function to misread. Noted here as a defense-in-depth gap in case a `Deny` ACE is ever introduced
/// by something other than this code path (e.g. third-party security tooling).
pub fn permissions_need_repair(path: &Path) -> bool {
    let Ok(admin_sid) = get_administrator_sid() else {
        // Can't even build the Administrators SID to compare against; err on the side of repairing.
        return true;
    };

    let path_wstr: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();

    unsafe {
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut security_descriptor = PSECURITY_DESCRIPTOR::default();
        if let Err(err) = GetNamedSecurityInfoW(
            PCWSTR(path_wstr.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&mut dacl),
            None,
            &mut security_descriptor,
        )
        .ok()
        {
            trace!(path = %path.display(), error = %err.message(), "cannot read DACL, flagging permissions for repair");
            return true;
        }
        if dacl.is_null() {
            // A NULL DACL grants everyone; re-stamp to lock it back down to Administrators-only.
            trace!(path = %path.display(), "NULL DACL (grants everyone), flagging permissions for repair");
            return true;
        }
        let mut acl_size_info: ACL_SIZE_INFORMATION = std::mem::zeroed();
        if let Err(err) = GetAclInformation(
            dacl,
            &mut acl_size_info as *mut _ as *mut _,
            mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        ) {
            trace!(path = %path.display(), error = %err.message(), "cannot read ACL info, flagging permissions for repair");
            return true;
        }

        // Conforming iff some `Allow` ACE already grants Administrators the full managed rights (and,
        // on a directory, is inheritable) — the policy lives in `grants_managed_admin_access`. If no
        // ACE qualifies (empty DACL, missing DELETE/execute, non-inheritable directory), repair.
        let conforming = (0..acl_size_info.AceCount).any(|i| {
            let mut ace_ptr: *mut ffi::c_void = ptr::null_mut();
            if let Err(err) = GetAce(dacl, i, &mut ace_ptr) {
                trace!(path = %path.display(), index = i, error = %err.message(), "GetAce failed while scanning DACL, skipping this ACE");
                return false;
            }
            let Some(ace) = ptr::NonNull::new(ace_ptr as *mut ACCESS_ALLOWED_ACE) else {
                return false;
            };
            grants_managed_admin_access(&*ace.as_ptr(), &admin_sid, path.is_dir())
        });

        if !conforming {
            trace!(path = %path.display(), ace_count = acl_size_info.AceCount, "DACL lacks a conforming Administrators ACE (empty, or missing DELETE/execute/inheritance), flagging permissions for repair");
        }
        !conforming
    }
}

/// Checks and repairs the managed Administrators-only permissions across `path` and everything
/// beneath it, re-stamping only the entries that are actually broken.
///
/// For each entry it checks (via [`permissions_need_repair`]) whether the DACL already grants the
/// managed Administrators access. It attempts to re-stamp only entries whose ACE are:
///
/// - Empty (denies everyone incl. `SYSTEM`) or `NULL`.
/// - Unreadable.
/// - Populated but insufficient (e.g. the old `Administrators:(R,W)` with no `DELETE` that blocks
///   decommission, or a non-inheritable directory ACE).
///
/// Conforming entries are left untouched, so a healthy tree is not rewritten on every startup. It
/// always recurses into directories to find broken children, and a broken directory is stamped
/// *before* its contents are listed so it becomes listable first. Agent Control owns these files, so
/// the rewrite succeeds even on an empty DACL.
///
/// Caveats (not currently hit in practice, but worth knowing before extending this):
///
/// - This walks the *whole* tree on every call, i.e. on every Agent Control startup, not just once
///   after an upgrade. [`permissions_need_repair`] keeps each individual check cheap (no re-stamping
///   of conforming entries), but a fleet with very large `filesystem/`/`fleet-data` trees still pays
///   one ACL read per entry on every restart, indefinitely.
/// - `path.is_dir()` and `read_dir` follow reparse points/symlinks, so a symlink planted inside a
///   managed tree would be traversed and re-ACL'd rather than skipped. Low risk given the tree is
///   Administrators-only to begin with, but there is no explicit guard against it.
pub fn ensure_permissions_recursive(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    if permissions_need_repair(path) {
        tracing::debug!(path = %path.display(), "repairing managed permissions");
        set_file_permissions_for_administrator(path).map_err(|err| {
            io::Error::other(format!(
                "setting windows permissions for {}: {err}",
                path.display()
            ))
        })?;
    } else {
        tracing::trace!(path = %path.display(), "managed permissions intact, skipping");
    }

    if path.is_dir() {
        std::fs::read_dir(path)?
            .try_for_each(|entry| ensure_permissions_recursive(&entry?.path()))?;
    }

    Ok(())
}

/// Runs [`ensure_permissions_recursive`] over each root in `roots`, repairing the managed
/// Administrators-only permissions across every root's tree (e.g. a sub-agent `filesystem/`, stored
/// remote configs under `fleet-data/`, local data).
///
/// Fails fast: the first root — or any entry within it — that cannot be repaired aborts the whole
/// call with that error, on the principle that a caller should not run on a data tree it cannot fully
/// access. A missing root is not an error.
pub fn ensure_managed_permissions<'a>(roots: impl IntoIterator<Item = &'a Path>) -> io::Result<()> {
    roots.into_iter().try_for_each(|root| {
        tracing::debug!(path = %root.display(), "ensuring correct ACL for managed root");
        ensure_permissions_recursive(root)
    })
}

#[cfg(test)]
#[allow(missing_docs)] // test-support code
pub mod tests {
    use std::fs;

    use windows::Win32::{
        Foundation::ERROR_SUCCESS,
        Security::{
            ACCESS_ALLOWED_ACE, ACL_SIZE_INFORMATION, AclSizeInformation,
            Authorization::GetNamedSecurityInfoW, CONTAINER_INHERIT_ACE, EqualSid, GetAce,
            GetAclInformation, INHERITED_ACE, NO_INHERITANCE, OBJECT_INHERIT_ACE,
        },
        Storage::FileSystem::{
            DELETE, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
        },
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
            CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                None,
                Some(PSID(admin_sid.as_mut_ptr() as *mut ffi::c_void)),
                &mut admin_sid_size,
            )
            .expect("Failed to create administrator SID");

            // Get file's DACL
            let path_wstr: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let mut dacl: *mut ACL = ptr::null_mut();
            let mut security_descriptor = PSECURITY_DESCRIPTOR::default();

            let result = GetNamedSecurityInfoW(
                PCWSTR(path_wstr.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(&mut dacl),
                None,
                &mut security_descriptor,
            );

            assert_eq!(result, ERROR_SUCCESS, "Failed to get security info");
            assert!(!dacl.is_null(), "DACL should not be null");

            // Verify exactly 1 ACE
            let mut acl_size_info: ACL_SIZE_INFORMATION = std::mem::zeroed();
            GetAclInformation(
                dacl,
                &mut acl_size_info as *mut _ as *mut _,
                mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
            .expect("Failed to get ACL information");
            assert_eq!(
                acl_size_info.AceCount, 1,
                "Should have exactly 1 ACE (Administrators only)"
            );

            // Verify ACE is for Administrators with Read/Write permissions
            let mut ace_ptr: *mut ffi::c_void = ptr::null_mut();
            GetAce(dacl, 0, &mut ace_ptr).expect("Failed to get ACE");

            let ace_ptr = ptr::NonNull::new(ace_ptr as *mut ACCESS_ALLOWED_ACE)
                .expect("ACE pointer should not be null");
            let ace = &*ace_ptr.as_ptr();
            let sid_in_ace = PSID(&ace.SidStart as *const u32 as *mut ffi::c_void);
            assert!(
                EqualSid(sid_in_ace, PSID(admin_sid.as_mut_ptr() as *mut ffi::c_void)).is_ok(),
                "ACE SID should match Administrators SID"
            );

            // The ACE grants the specific file rights read/write/execute plus DELETE. Execute is
            // required so executables inheriting this ACE (e.g. the self-update binary) can be spawned.
            let expected_mask =
                (FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_GENERIC_EXECUTE | DELETE).0;
            assert_eq!(
                ace.Mask, expected_mask,
                "ACE should have FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_GENERIC_EXECUTE, and DELETE permissions"
            );

            // Inheritance flags only make sense on containers: for a directory the ACE must be
            // inheritable so files and subdirectories a sub-agent creates at runtime inside it
            // (e.g. newrelic-infra.log) inherit Administrators access instead of getting an empty
            // DACL that denies everyone. Windows strips OI|CI from ACEs on leaf files, so this is
            // asserted for directories only.
            if path.is_dir() {
                let expected_flags = (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE).0 as u8;
                assert_eq!(
                    ace.Header.AceFlags & expected_flags,
                    expected_flags,
                    "directory ACE should be inheritable by child objects and containers (OI|CI)"
                );
            }
        }
    }

    /// Asserts the object at `path` has at least one ACE that was **inherited** from its parent and
    /// grants Administrators. This is the exact property the fix provides: a file a sub-agent creates
    /// at runtime inside a managed directory inherits Administrators access. On the pre-fix
    /// (non-inheritable) code nothing is inherited, so no ACE carries the INHERITED flag and this
    /// fails — which a functional read/write cannot detect, because an admin creator gets a
    /// permissive token-default DACL whenever nothing is inherited.
    fn assert_inherited_admin_ace(path: &Path) {
        let mut admin_sid_size = SECURITY_MAX_SID_SIZE;
        let mut admin_sid: Vec<u8> = vec![0; admin_sid_size as usize];

        unsafe {
            CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                None,
                Some(PSID(admin_sid.as_mut_ptr() as *mut ffi::c_void)),
                &mut admin_sid_size,
            )
            .expect("Failed to create administrator SID");

            let path_wstr: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let mut dacl: *mut ACL = ptr::null_mut();
            let mut security_descriptor = PSECURITY_DESCRIPTOR::default();
            let result = GetNamedSecurityInfoW(
                PCWSTR(path_wstr.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(&mut dacl),
                None,
                &mut security_descriptor,
            );
            assert_eq!(result, ERROR_SUCCESS, "Failed to get child security info");
            assert!(!dacl.is_null(), "child DACL should not be null");

            let mut acl_size_info: ACL_SIZE_INFORMATION = mem::zeroed();
            GetAclInformation(
                dacl,
                &mut acl_size_info as *mut _ as *mut _,
                mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
            .expect("Failed to get child ACL information");

            let mut inherited_admin = false;
            for i in 0..acl_size_info.AceCount {
                let mut ace_ptr: *mut ffi::c_void = ptr::null_mut();
                if GetAce(dacl, i, &mut ace_ptr).is_err() {
                    continue;
                }
                // Guard the raw pointer with NonNull before dereferencing (mirrors
                // assert_windows_permissions and satisfies CodeQL's invalid-pointer check).
                let Some(ace_ptr) = ptr::NonNull::new(ace_ptr as *mut ACCESS_ALLOWED_ACE) else {
                    continue;
                };
                let ace = &*ace_ptr.as_ptr();
                let is_inherited = (ace.Header.AceFlags & (INHERITED_ACE.0 as u8)) != 0;
                let sid_in_ace = PSID(&ace.SidStart as *const u32 as *mut ffi::c_void);
                let is_admin =
                    EqualSid(sid_in_ace, PSID(admin_sid.as_mut_ptr() as *mut ffi::c_void)).is_ok();
                if is_inherited && is_admin {
                    inherited_admin = true;
                    break;
                }
            }
            assert!(
                inherited_admin,
                "child must have an INHERITED Administrators ACE — proof it inherited the managed \
                 directory's access. Pre-fix (non-inheritable) code inherits nothing, so this fails."
            );
        }
    }

    /// Behavioral regression test for the bug this change fixes.
    ///
    /// Agent Control hardens a directory, then at runtime a sub-agent creates files inside it (e.g.
    /// `newrelic-infra.log`) with no permissions of their own — they must *inherit* access from the
    /// directory. Before the ACE was made inheritable, such a child inherited nothing, which on the
    /// real system left the sub-agent unable to open its log ("Access is denied").
    ///
    /// The other tests only assert the ACL of objects `set_file_permissions_for_administrator` was
    /// called on directly, so inheritance to a runtime-created child was never exercised — which is
    /// why this went uncaught. Note we assert the child *inherited* the Administrators ACE rather
    /// than a functional read/write: in the CI admin context an un-inherited child still gets a
    /// permissive token-default DACL, so functional access alone cannot distinguish the bug.
    #[test]
    fn child_created_in_managed_directory_inherits_admin_access() {
        let tempdir = tempfile::tempdir().unwrap();
        let managed_dir = tempdir.path().join("nr-infra");
        fs::create_dir(&managed_dir).unwrap();

        // Harden the directory the way Agent Control does for its managed filesystem.
        set_file_permissions_for_administrator(&managed_dir)
            .expect("hardening the directory should succeed");

        // A sub-agent creates its log inside the managed directory (no permissions of its own).
        let child = managed_dir.join("newrelic-infra.log");
        fs::write(&child, b"heartbeat").expect("creating the child file should succeed");

        // It must have inherited the Administrators ACE from the directory. Pre-fix this fails
        // because the directory's ACE was not inheritable.
        assert_inherited_admin_ace(&child);
    }

    /// Reproduces the hardening an **older (pre-fix) Agent Control** applied: a PROTECTED,
    /// Administrators-only DACL whose ACE is **non-inheritable** (`NO_INHERITANCE`). Kept in the test
    /// module only, to recreate on disk the exact state older versions left behind so we can prove the
    /// current (inheritable) fix heals it. The specific rights granted are irrelevant to the bug —
    /// `PROTECTED` + `NO_INHERITANCE` is the invariant that makes an existing inherited-only child
    /// collapse to an empty DACL under `SetNamedSecurityInfoW` propagation.
    fn legacy_harden_non_inheritable(path: &Path) {
        let path_wstr: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let admin_sid = get_administrator_sid().expect("failed to get administrator SID");
        let trustee = TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            ptstrName: PWSTR(admin_sid.as_ptr() as *mut u16),
            ..Default::default()
        };
        let access_entry = EXPLICIT_ACCESS_W {
            grfAccessPermissions: (FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE).0,
            grfAccessMode: SET_ACCESS,
            grfInheritance: NO_INHERITANCE,
            Trustee: trustee,
        };
        unsafe {
            let mut acl: *mut ACL = ptr::null_mut();
            SetEntriesInAclW(Some(&[access_entry]), None, &mut acl)
                .ok()
                .expect("legacy SetEntriesInAclW failed");
            SetNamedSecurityInfoW(
                PCWSTR(path_wstr.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(acl as *const ACL),
                None,
            )
            .ok()
            .expect("legacy SetNamedSecurityInfoW failed");
        }
    }

    /// Asserts the object at `path` has a present-but-**empty** DACL (zero ACEs) — the state that
    /// denies every principal, including the LocalSystem process that created the file. This is the
    /// exact broken state older versions left runtime-created children in (measured on the canary as
    /// SDDL `O:BAG:SYD:AI`). A *null* DACL is explicitly rejected: that grants everyone and would be a
    /// different (non-reproducing) state, so we fail loudly rather than pass on it.
    fn assert_dacl_is_empty(path: &Path) {
        unsafe {
            let path_wstr: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let mut dacl: *mut ACL = ptr::null_mut();
            let mut security_descriptor = PSECURITY_DESCRIPTOR::default();
            GetNamedSecurityInfoW(
                PCWSTR(path_wstr.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(&mut dacl),
                None,
                &mut security_descriptor,
            )
            .ok()
            .expect("Failed to get child security info");
            assert!(
                !dacl.is_null(),
                "expected a present (empty) DACL; a null DACL would grant everyone — the repro is wrong"
            );

            let mut acl_size_info: ACL_SIZE_INFORMATION = mem::zeroed();
            GetAclInformation(
                dacl,
                &mut acl_size_info as *mut _ as *mut _,
                mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
            .expect("Failed to get child ACL information");
            assert_eq!(
                acl_size_info.AceCount, 0,
                "expected an EMPTY DACL (0 ACEs) reproducing the pre-fix propagation wipe; found {} ACE(s)",
                acl_size_info.AceCount
            );
        }
    }

    /// Regression test for the **heal** path: a runtime-created child left with an empty DACL by an
    /// older (pre-fix) Agent Control must regain Administrators access after the fix runs.
    ///
    /// This reproduces the true production state through the genuine pre-fix mechanism
    /// (`legacy_harden_non_inheritable`, which wipes the inherited-only child to an empty DACL), then
    /// proves the repair heals it. The repair is `ensure_permissions_recursive`, which re-stamps the
    /// Administrators DACL directly onto every existing entry — it does NOT rely on inheritance
    /// propagation reaching an already-empty child. (In the field an upgrade did not heal, because
    /// `ensure_dir` never re-hardened the existing managed dir at all — see NR-601065 — so relying on
    /// propagation was doubly unsafe.) The `assert_dacl_is_empty` checkpoint guarantees we actually
    /// recreated the empty-DACL state and did not silently fall back to the token-default path.
    #[test]
    fn recursive_repair_heals_existing_empty_dacl_child_from_older_version() {
        let tempdir = tempfile::tempdir().unwrap();
        let managed_dir = tempdir.path().join("nr-infra");
        fs::create_dir(&managed_dir).unwrap();

        // Start from the fixed state so the child is created inheritance-accepting (D:AI,
        // inherited-only) — the precondition for the propagation wipe. Creating the child after a
        // *non*-inheritable harden would instead give it an explicit token-default DACL, which never
        // collapses to empty (and is why windows-latest cannot reproduce the production symptom).
        set_file_permissions_for_administrator(&managed_dir)
            .expect("initial (fixed) hardening should succeed");
        let child = managed_dir.join("newrelic-infra.log");
        fs::write(&child, b"heartbeat").expect("creating the child file should succeed");
        assert_inherited_admin_ace(&child); // healthy start: inherited the directory's Administrators ACE

        // Reproduce what an older Agent Control version did: re-harden the directory with a
        // non-inheritable, protected ACE. `SetNamedSecurityInfoW` propagation recomputes the existing
        // inherited-only child from a parent that now has nothing inheritable, collapsing its DACL to
        // empty — denying everyone, including the LocalSystem sub-agent.
        legacy_harden_non_inheritable(&managed_dir);
        assert_dacl_is_empty(&child);
        assert!(
            fs::OpenOptions::new().write(true).open(&child).is_err(),
            "an empty-DACL child must be unopenable for write — this is the sub-agent's 'Access is denied'"
        );

        // The fix: recursively re-stamp the managed permissions. This rewrites the empty child's DACL
        // directly (Agent Control owns it, so this works even on an empty DACL) — no reliance on
        // inheritance propagation reaching the child. This is what runs on the next sub-agent start
        // after upgrading to the fixed agent-control.
        ensure_permissions_recursive(&managed_dir)
            .expect("recursive permission repair should succeed");
        assert_windows_permissions(&child); // child now carries its own Administrators entry
        assert!(
            fs::OpenOptions::new().write(true).open(&child).is_ok(),
            "after the repair the child must grant Administrators access and be openable"
        );
    }

    /// `permissions_need_repair` is the gate that keeps the startup repair from re-stamping a healthy
    /// tree on every boot. It must treat healthy entries as fine and flag only broken (empty) ones.
    #[test]
    fn permissions_need_repair_flags_only_broken_entries() {
        let tempdir = tempfile::tempdir().unwrap();
        let dir = tempdir.path().join("nr-infra");
        fs::create_dir(&dir).unwrap();
        set_file_permissions_for_administrator(&dir).expect("hardening should succeed");

        // A freshly-hardened directory, and a child that inherits its ACE, are both healthy.
        assert!(
            !permissions_need_repair(&dir),
            "a freshly-hardened directory must not be flagged for repair"
        );
        let child = dir.join("newrelic-infra.log");
        fs::write(&child, b"heartbeat").expect("creating child should succeed");
        assert!(
            !permissions_need_repair(&child),
            "a child inheriting the Administrators ACE must not be flagged for repair"
        );

        // Reproduce the older-version wipe; the now-empty child must be flagged.
        legacy_harden_non_inheritable(&dir);
        assert_dacl_is_empty(&child);
        assert!(
            permissions_need_repair(&child),
            "an empty-DACL child must be flagged for repair"
        );
    }

    /// The production wipe hit *directories* (`data`, `user_data`) and files nested beneath the
    /// managed root, not just a direct child. This proves the recursive repair (a) re-stamps an
    /// empty-DACL directory and can then *list* it to descend, and (b) reaches a file nested below.
    #[test]
    fn recursive_repair_heals_nested_empty_dacl_directory_and_file() {
        let tempdir = tempfile::tempdir().unwrap();
        let managed_dir = tempdir.path().join("nr-infra");
        fs::create_dir(&managed_dir).unwrap();
        set_file_permissions_for_administrator(&managed_dir).expect("hardening should succeed");

        // A sub-agent creates a nested data directory with a file inside; both inherit (healthy).
        let subdir = managed_dir.join("newrelic-infra");
        fs::create_dir(&subdir).unwrap();
        let nested_file = subdir.join("newrelic-infra.log");
        fs::write(&nested_file, b"heartbeat").expect("creating nested file should succeed");

        // An older version re-hardens the managed dir non-inheritably; propagation wipes the whole
        // inherited-only subtree — the nested directory *and* the file within it — to empty DACLs.
        legacy_harden_non_inheritable(&managed_dir);
        assert_dacl_is_empty(&subdir);
        assert_dacl_is_empty(&nested_file);

        ensure_permissions_recursive(&managed_dir)
            .expect("recursive permission repair should succeed");

        // The empty directory was repaired (hence listable, so recursion could descend into it), and
        // the file nested beneath it was healed too.
        assert_windows_permissions(&subdir);
        assert_windows_permissions(&nested_file);
        assert!(
            fs::OpenOptions::new()
                .write(true)
                .open(&nested_file)
                .is_ok(),
            "the nested file must be openable after the recursive repair"
        );
    }

    /// The decommission-cleanup fix relies on the repair restoring DELETE: an empty-DACL file grants
    /// delete to no one, so removing the managed tree fails ("Access is denied") until it is repaired.
    #[test]
    fn repair_restores_delete_so_managed_tree_is_removable() {
        let tempdir = tempfile::tempdir().unwrap();
        let managed_dir = tempdir.path().join("nr-infra");
        fs::create_dir(&managed_dir).unwrap();
        set_file_permissions_for_administrator(&managed_dir).expect("hardening should succeed");
        let child = managed_dir.join("newrelic-infra.log");
        fs::write(&child, b"heartbeat").expect("creating child should succeed");

        // Older-version wipe: the child's DACL is now empty (grants DELETE to no one).
        legacy_harden_non_inheritable(&managed_dir);
        assert_dacl_is_empty(&child);

        // The repair re-stamps DELETE (via the Administrators ACE) across the tree.
        ensure_permissions_recursive(&managed_dir)
            .expect("recursive permission repair should succeed");

        // Cleanup now succeeds where a decommission previously failed with os error 5.
        fs::remove_dir_all(&managed_dir)
            .expect("managed tree must be removable after the permission repair");
        assert!(!managed_dir.exists());
    }

    /// Reproduces the permissions an *even older* agent-control applied to stored configs: a PROTECTED
    /// Administrators-only ACE granting only read+write (mask `0x12019f`) with **no DELETE** and no
    /// execute. This is the state that leaves a stored remote config undeletable on decommission
    /// (NR-601065). Non-inheritable, matching what was observed in the field.
    fn legacy_harden_read_write_only(path: &Path) {
        let path_wstr: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let admin_sid = get_administrator_sid().expect("failed to get administrator SID");
        let trustee = TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            ptstrName: PWSTR(admin_sid.as_ptr() as *mut u16),
            ..Default::default()
        };
        let access_entry = EXPLICIT_ACCESS_W {
            grfAccessPermissions: (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0, // no DELETE, no EXECUTE
            grfAccessMode: SET_ACCESS,
            grfInheritance: NO_INHERITANCE,
            Trustee: trustee,
        };
        unsafe {
            let mut acl: *mut ACL = ptr::null_mut();
            SetEntriesInAclW(Some(&[access_entry]), None, &mut acl)
                .ok()
                .expect("legacy SetEntriesInAclW failed");
            SetNamedSecurityInfoW(
                PCWSTR(path_wstr.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(acl as *const ACL),
                None,
            )
            .ok()
            .expect("legacy SetNamedSecurityInfoW failed");
        }
    }

    /// Regression for the decommission-cleanup failure (NR-601065): a stored remote config left by an
    /// older version with `Administrators:(R,W)` and **no DELETE** cannot be removed (`os error 5`),
    /// even though its DACL is non-empty. The repair must detect it as insufficient (not just empty)
    /// and re-stamp the full managed rights, restoring DELETE.
    #[test]
    fn repair_reinstates_delete_on_read_write_only_file_from_older_version() {
        let tempdir = tempfile::tempdir().unwrap();
        let managed_dir = tempdir.path().join("fleet-data");
        fs::create_dir(&managed_dir).unwrap();
        set_file_permissions_for_administrator(&managed_dir).expect("hardening should succeed");
        let cfg = managed_dir.join("remote_config.yaml");
        fs::write(&cfg, b"config: value").expect("creating config should succeed");

        // An older agent-control stamped it read/write only (no DELETE) - the field state.
        legacy_harden_read_write_only(&cfg);
        // Populated but insufficient: the empty-only check missed this; it must now be flagged.
        assert!(
            permissions_need_repair(&cfg),
            "a read/write-only config lacking DELETE must be flagged for repair"
        );

        // The repair re-stamps the full managed rights (Modify, which includes DELETE).
        ensure_permissions_recursive(&managed_dir)
            .expect("recursive permission repair should succeed");
        assert_windows_permissions(&cfg); // now Administrators Modify (read/write/execute/delete)

        // And the config is now deletable, so decommission cleanup succeeds.
        fs::remove_file(&cfg).expect("config must be deletable after the repair");
    }

    /// A managed *directory* an older version hardened non-inheritably (`Administrators:(R,W,D)`, no
    /// `OI|CI`, mask `0x13019f`) is not empty, so the empty-only check skipped it — yet it is exactly
    /// what leaves runtime children unable to inherit access. The broadened repair must flag it (a
    /// directory's managed ACE must grant the full rights *and* be inheritable) and re-stamp it, so
    /// future runtime children inherit administrator access.
    #[test]
    fn repair_makes_a_non_inheritable_managed_directory_inheritable() {
        let tempdir = tempfile::tempdir().unwrap();
        let dir = tempdir.path().join("newrelic-infra");
        fs::create_dir(&dir).unwrap();

        // Older version: protected Administrators (R,W,D) but NON-inheritable (no OI|CI).
        legacy_harden_non_inheritable(&dir);
        assert!(
            permissions_need_repair(&dir),
            "a non-inheritable managed directory must be flagged for repair"
        );

        ensure_permissions_recursive(&dir).expect("recursive permission repair should succeed");

        // assert_windows_permissions requires a directory's ACE to grant the full managed rights AND
        // be inheritable (OI|CI), so this proves the directory was normalized.
        assert_windows_permissions(&dir);
    }
}
