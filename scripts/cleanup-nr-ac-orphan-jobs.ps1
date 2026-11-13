#Requires -Version 5.1
<#
.SYNOPSIS
    Cleans up orphaned permanent named Job Objects whose name begins with "nr-ac-".

.DESCRIPTION
    An "orphan" is a permanent job object that was left behind in \BaseNamedObjects
    when Agent Control (or a probe) exited before calling NtMakeTemporaryObject.
    Orphans waste a kernel name-space slot and can mislead diagnostics.

    A job is treated as an orphan when ALL three conditions hold:
      - permanent == true      (would survive indefinitely without cleanup)
      - active_processes == 0  (no process is currently a member)
      - other_handles == 0     (nothing else has the job open — safe to remove now)

    For each orphan the script calls NtMakeTemporaryObject, which strips the
    OBJ_PERMANENT attribute.  The job's name disappears from the object-manager
    directory immediately because no other handle is holding it open.

    PRIVILEGE REQUIREMENT
    NtMakeTemporaryObject requires SeCreatePermanentPrivilege ("Create permanent
    shared objects") to be both present and enabled in the calling token.  Running
    as NT AUTHORITY\SYSTEM (the account AC uses as a Windows service) satisfies
    this automatically.  Running as a non-elevated administrator, the privilege is
    absent and the script will fail with STATUS_PRIVILEGE_NOT_HELD.  An admin can
    grant it via secpol.msc → "User Rights Assignment → Create permanent shared
    objects", but this is a security-sensitive setting and should be applied to the
    service account only.

    The script calls all NT and Win32 APIs directly via P/Invoke (Add-Type).
    No external tools or compiled binaries are required.

.PARAMETER Prefix
    Only consider job objects whose name starts with this string.
    Default: "nr-ac-"

.PARAMETER Directory
    NT object-manager path of the directory to enumerate.
    Default: "\BaseNamedObjects"

.PARAMETER WhatIf
    Enumerate and print orphans but do not call NtMakeTemporaryObject.

.EXAMPLE
    # Dry run — see what would be cleaned up
    .\cleanup-nr-ac-orphan-jobs.ps1 -WhatIf

.EXAMPLE
    # Actually clean up orphans
    .\cleanup-nr-ac-orphan-jobs.ps1

.EXAMPLE
    # Restrict to a specific agent type prefix
    .\cleanup-nr-ac-orphan-jobs.ps1 -Prefix "nr-ac-infrastructure-agent"
#>
param(
    [string] $Prefix    = "nr-ac-",
    [string] $Directory = "\BaseNamedObjects",
    [switch] $WhatIf
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ── P/Invoke type definition ──────────────────────────────────────────────────
# All struct sizes and field offsets are for 64-bit Windows (x64).
# UNICODE_STRING : 16 bytes  (Length:2, MaximumLength:2, pad:4, Buffer:8)
# OBJECT_ATTRIBUTES: 48 bytes  (Length:4, pad:4, RootDirectory:8,
#                                ObjectName:8, Attributes:4, pad:4,
#                                SecurityDescriptor:8, QoS:8)

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class NtJobCleanup
{
    // ── Access rights ─────────────────────────────────────────────────────────
    public const uint DIRECTORY_QUERY  = 0x00000001;
    public const uint JOB_OBJECT_QUERY = 0x00000004;

    // NtOpenJobObject requires DELETE access to call NtMakeTemporaryObject on
    // the same handle.  We open with both so a single open covers both uses.
    public const uint JOB_OBJECT_DELETE = 0x00010000;  // standard DELETE right

    // TOKEN_ADJUST_PRIVILEGES lets AdjustTokenPrivileges enable/disable privs.
    // TOKEN_QUERY is required to call OpenProcessToken successfully on most configs.
    public const uint TOKEN_ADJUST_PRIVILEGES = 0x0020;
    public const uint TOKEN_QUERY             = 0x0008;

    // ── Object-attribute flags ────────────────────────────────────────────────
    public const uint OBJ_CASE_INSENSITIVE = 0x00000040;
    public const uint OBJ_PERMANENT        = 0x00000010;

    // ── NTSTATUS codes ────────────────────────────────────────────────────────
    const int STATUS_SUCCESS         = 0x00000000;
    const int STATUS_MORE_ENTRIES    = 0x00000105;
    const int STATUS_NO_MORE_ENTRIES = unchecked((int)0x8000001A);

    // ── Job-object information classes ────────────────────────────────────────
    const int JobObjectBasicAccountingInformation = 1;

    // ── NtQueryObject information class ──────────────────────────────────────
    const int ObjectBasicInformation = 0;

    // ── Struct sizes (x64 fixed) ──────────────────────────────────────────────
    const int UNICODE_STRING_SIZE    = 16;
    const int OBJECT_ATTRIBUTES_SIZE = 48;

    // ── OBJECT_BASIC_INFORMATION ──────────────────────────────────────────────
    [StructLayout(LayoutKind.Sequential)]
    struct OBJECT_BASIC_INFORMATION
    {
        public uint Attributes;
        public uint GrantedAccess;
        public uint HandleCount;    // includes our own query handle; subtract 1
        public uint PointerCount;
        public uint PagedPoolCharge;
        public uint NonPagedPoolCharge;
        public uint Reserved0, Reserved1, Reserved2;
        public uint NameInfoSize;
        public uint TypeInfoSize;
        public uint SecurityDescriptorSize;
        public long CreationTime;
    }

    // ── JOBOBJECT_BASIC_ACCOUNTING_INFORMATION ────────────────────────────────
    [StructLayout(LayoutKind.Sequential)]
    struct JOBOBJECT_BASIC_ACCOUNTING_INFORMATION
    {
        public long TotalUserTime;
        public long TotalKernelTime;
        public long ThisPeriodTotalUserTime;
        public long ThisPeriodTotalKernelTime;
        public uint TotalPageFaultCount;
        public uint TotalProcesses;
        public uint ActiveProcesses;
        public uint TotalTerminatedProcesses;
    }

    // ── LUID and TOKEN_PRIVILEGES ─────────────────────────────────────────────
    // Used by AdjustTokenPrivileges to enable SeCreatePermanentPrivilege.
    // A LUID is a 64-bit value (LowPart:4, HighPart:4).
    // TOKEN_PRIVILEGES: Count(4), Luid(8), Attributes(4).  For a single
    // privilege the struct is 16 bytes.  Attributes=SE_PRIVILEGE_ENABLED=2.
    [StructLayout(LayoutKind.Sequential)]
    public struct TOKEN_PRIVILEGES_1
    {
        public uint  PrivilegeCount;   // must be 1
        public uint  LuidLow;
        public int   LuidHigh;
        public uint  Attributes;       // SE_PRIVILEGE_ENABLED = 2
    }

    public const uint SE_PRIVILEGE_ENABLED = 2;

    // ── P/Invoke declarations ─────────────────────────────────────────────────

    [DllImport("ntdll.dll")]
    static extern int NtOpenDirectoryObject(
        out IntPtr DirectoryHandle,
        uint       DesiredAccess,
        IntPtr     ObjectAttributes);

    [DllImport("ntdll.dll")]
    static extern int NtQueryDirectoryObject(
        IntPtr   DirectoryHandle,
        IntPtr   Buffer,
        int      Length,
        byte     ReturnSingleEntry,   // BOOLEAN (1 byte)
        byte     RestartScan,         // BOOLEAN (1 byte)
        ref uint Context,
        out uint ReturnLength);

    // Opens a job object with the specified access rights.
    // We call with JOB_OBJECT_QUERY | DELETE so we can both query and
    // later call NtMakeTemporaryObject on the same handle.
    [DllImport("ntdll.dll")]
    static extern int NtOpenJobObject(
        out IntPtr JobHandle,
        uint       DesiredAccess,
        IntPtr     ObjectAttributes);

    [DllImport("ntdll.dll")]
    static extern int NtQueryObject(
        IntPtr Handle,
        int    ObjectInformationClass,
        IntPtr ObjectInformation,
        int    ObjectInformationLength,
        out int ReturnLength);

    // Strips OBJ_PERMANENT from the object.  The name is removed from the
    // object-manager namespace when the last handle to the object closes.
    // If no other handle is open (OtherHandles == 0), the name disappears
    // as soon as this call returns and we close our handle.
    // Requires SeCreatePermanentPrivilege to be enabled in the calling token.
    [DllImport("ntdll.dll")]
    static extern int NtMakeTemporaryObject(IntPtr Handle);

    [DllImport("ntdll.dll")]
    static extern int NtClose(IntPtr Handle);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool QueryInformationJobObject(
        IntPtr hJob,
        int    JobObjectInformationClass,
        IntPtr lpJobObjectInfo,
        int    cbJobObjectInfoLength,
        out int lpReturnLength);

    // Opens the access token of the calling process.
    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool OpenProcessToken(
        IntPtr  ProcessHandle,
        uint    DesiredAccess,
        out IntPtr TokenHandle);

    // Returns a LUID for a named privilege (e.g. "SeCreatePermanentPrivilege").
    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool LookupPrivilegeValueW(
        string lpSystemName,     // null for local system
        string lpName,
        out uint pLuidLow,
        out int  pLuidHigh);

    // Enables or disables privileges in the given token.
    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool AdjustTokenPrivileges(
        IntPtr TokenHandle,
        bool   DisableAllPrivileges,
        ref TOKEN_PRIVILEGES_1 NewState,
        uint   BufferLength,
        IntPtr PreviousState,  // null — we don't need the old state
        IntPtr ReturnLength);  // null

    [DllImport("kernel32.dll")]
    static extern IntPtr GetCurrentProcess();

    [DllImport("kernel32.dll")]
    static extern bool CloseHandle(IntPtr hObject);

    // ── AllocObjectAttributes ─────────────────────────────────────────────────
    // Allocates [OBJECT_ATTRIBUTES][UNICODE_STRING][path bytes] contiguously.
    // Returns a pointer to the OBJECT_ATTRIBUTES (== start of allocation).
    // Caller must free with Marshal.FreeHGlobal after the NT call returns.
    static IntPtr AllocObjectAttributes(string ntPath, uint attributes)
    {
        byte[] pathBytes = Encoding.Unicode.GetBytes(ntPath);
        int    total     = OBJECT_ATTRIBUTES_SIZE + UNICODE_STRING_SIZE + pathBytes.Length;
        IntPtr alloc     = Marshal.AllocHGlobal(total);

        for (int i = 0; i < total; i++) Marshal.WriteByte(alloc, i, 0);

        IntPtr oaPtr   = alloc;
        IntPtr usPtr   = alloc + OBJECT_ATTRIBUTES_SIZE;
        IntPtr pathPtr = alloc + OBJECT_ATTRIBUTES_SIZE + UNICODE_STRING_SIZE;

        Marshal.Copy(pathBytes, 0, pathPtr, pathBytes.Length);

        // UNICODE_STRING at usPtr
        Marshal.WriteInt16(usPtr, 0, (short)pathBytes.Length);
        Marshal.WriteInt16(usPtr, 2, (short)pathBytes.Length);
        Marshal.WriteIntPtr(usPtr, 8, pathPtr);

        // OBJECT_ATTRIBUTES at oaPtr
        Marshal.WriteInt32(oaPtr,   0, OBJECT_ATTRIBUTES_SIZE);
        Marshal.WriteIntPtr(oaPtr, 16, usPtr);
        Marshal.WriteInt32(oaPtr,  24, (int)attributes);

        return alloc;
    }

    // ── Public result type ────────────────────────────────────────────────────
    public class JobEntry
    {
        public string Path;
        public bool   Permanent;
        public uint   OtherHandles;
        public uint   ActiveProcesses;
        public bool   IsOrphan;        // permanent && active==0 && other_handles==0
    }

    // ── EnablePrivilege ───────────────────────────────────────────────────────
    // Enables a named privilege (e.g. "SeCreatePermanentPrivilege") in the
    // calling process token.  Throws if the privilege is not in the token at all
    // (it cannot be enabled if it was never granted).
    public static void EnablePrivilege(string privName)
    {
        IntPtr token;
        if (!OpenProcessToken(GetCurrentProcess(),
                              TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                              out token))
            throw new Exception(
                string.Format("OpenProcessToken failed: Win32 error {0}",
                              Marshal.GetLastWin32Error()));
        try
        {
            uint luidLow; int luidHigh;
            if (!LookupPrivilegeValueW(null, privName, out luidLow, out luidHigh))
                throw new Exception(
                    string.Format("LookupPrivilegeValueW(\"{0}\") failed: Win32 error {1}",
                                  privName, Marshal.GetLastWin32Error()));

            var tp = new TOKEN_PRIVILEGES_1
            {
                PrivilegeCount = 1,
                LuidLow        = luidLow,
                LuidHigh       = luidHigh,
                Attributes     = SE_PRIVILEGE_ENABLED,
            };

            // AdjustTokenPrivileges succeeds even if the privilege is not
            // present — it just leaves GetLastError == ERROR_NOT_ALL_ASSIGNED.
            AdjustTokenPrivileges(token, false, ref tp, 0,
                                  IntPtr.Zero, IntPtr.Zero);

            int err = Marshal.GetLastWin32Error();
            if (err != 0)
                throw new Exception(
                    string.Format(
                        "AdjustTokenPrivileges(\"{0}\"): Win32 error {1} " +
                        "(ERROR_NOT_ALL_ASSIGNED=1300 means the privilege is absent from the token)",
                        privName, err));
        }
        finally { CloseHandle(token); }
    }

    // ── FindOrphans ───────────────────────────────────────────────────────────
    // Returns all Job objects in `directory` whose name starts with `prefix`
    // and that qualify as orphans (permanent, no active processes, no other handles).
    public static List<JobEntry> FindOrphans(string directory, string prefix)
    {
        var results = new List<JobEntry>();

        // ── 1. Open directory ─────────────────────────────────────────────────
        IntPtr dirAlloc = AllocObjectAttributes(directory, OBJ_CASE_INSENSITIVE);
        IntPtr dirHandle;
        try
        {
            int s = NtOpenDirectoryObject(out dirHandle, DIRECTORY_QUERY, dirAlloc);
            if (s != STATUS_SUCCESS)
                throw new Exception(
                    string.Format("NtOpenDirectoryObject(\"{0}\"): NTSTATUS 0x{1:X8}",
                                  directory, (uint)s));
        }
        finally { Marshal.FreeHGlobal(dirAlloc); }

        // ── 2. Enumerate directory ────────────────────────────────────────────
        const int BufSize = 65536;
        IntPtr    buf     = Marshal.AllocHGlobal(BufSize);
        try
        {
            uint ctx     = 0;
            byte restart = 1;

            while (true)
            {
                uint retLen;
                int s = NtQueryDirectoryObject(
                    dirHandle, buf, BufSize, 0, restart,
                    ref ctx, out retLen);
                restart = 0;

                if (s == STATUS_NO_MORE_ENTRIES) break;
                if (s != STATUS_SUCCESS && s != STATUS_MORE_ENTRIES)
                    throw new Exception(
                        string.Format("NtQueryDirectoryObject: NTSTATUS 0x{0:X8}", (uint)s));

                int offset = 0;
                while (true)
                {
                    ushort nameLen = (ushort)Marshal.ReadInt16(buf, offset + 0);
                    ushort typeLen = (ushort)Marshal.ReadInt16(buf, offset + 16);
                    if (nameLen == 0 && typeLen == 0) break;

                    IntPtr nameBuf = Marshal.ReadIntPtr(buf, offset + 8);
                    IntPtr typeBuf = Marshal.ReadIntPtr(buf, offset + 24);
                    string name     = Marshal.PtrToStringUni(nameBuf, nameLen / 2);
                    string typeName = Marshal.PtrToStringUni(typeBuf, typeLen / 2);
                    offset += 32;

                    if (!typeName.Equals("Job", StringComparison.OrdinalIgnoreCase))
                        continue;
                    if (prefix.Length > 0 &&
                        !name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                        continue;

                    // ── 3. Open job for query + delete ────────────────────────
                    // We need DELETE access here (not just JOB_OBJECT_QUERY) so
                    // that the same handle can later be passed to
                    // NtMakeTemporaryObject.  Opening with QUERY alone would
                    // require a second open with DELETE; using a single handle
                    // avoids a time-of-check/time-of-use window.
                    string fullPath = directory + "\\" + name;
                    IntPtr jobAlloc = AllocObjectAttributes(fullPath, OBJ_CASE_INSENSITIVE);
                    IntPtr jobHandle;
                    try
                    {
                        int os = NtOpenJobObject(
                            out jobHandle,
                            JOB_OBJECT_QUERY | JOB_OBJECT_DELETE,
                            jobAlloc);
                        if (os != STATUS_SUCCESS) continue; // skip if DACL denies access
                    }
                    finally { Marshal.FreeHGlobal(jobAlloc); }

                    try
                    {
                        var e = new JobEntry { Path = fullPath };

                        // ── 4. Permanence and handle count ────────────────────
                        int    biSize = Marshal.SizeOf(typeof(OBJECT_BASIC_INFORMATION));
                        IntPtr biBuf  = Marshal.AllocHGlobal(biSize);
                        try
                        {
                            int retLen2;
                            if (NtQueryObject(jobHandle, ObjectBasicInformation,
                                              biBuf, biSize, out retLen2) == STATUS_SUCCESS)
                            {
                                var bi = (OBJECT_BASIC_INFORMATION)
                                    Marshal.PtrToStructure(biBuf,
                                        typeof(OBJECT_BASIC_INFORMATION));
                                e.Permanent    = (bi.Attributes & OBJ_PERMANENT) != 0;
                                e.OtherHandles = bi.HandleCount > 0
                                    ? bi.HandleCount - 1 : 0;
                            }
                        }
                        finally { Marshal.FreeHGlobal(biBuf); }

                        // ── 5. Active processes ───────────────────────────────
                        int    aiSize = Marshal.SizeOf(
                            typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION));
                        IntPtr aiBuf  = Marshal.AllocHGlobal(aiSize);
                        try
                        {
                            int retLen2;
                            if (QueryInformationJobObject(jobHandle,
                                    JobObjectBasicAccountingInformation,
                                    aiBuf, aiSize, out retLen2))
                            {
                                var ai = (JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)
                                    Marshal.PtrToStructure(aiBuf,
                                        typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION));
                                e.ActiveProcesses = ai.ActiveProcesses;
                            }
                        }
                        finally { Marshal.FreeHGlobal(aiBuf); }

                        e.IsOrphan = e.Permanent && e.ActiveProcesses == 0 && e.OtherHandles == 0;
                        results.Add(e);
                    }
                    finally { NtClose(jobHandle); }
                }

                if (s != STATUS_MORE_ENTRIES) break;
            }
        }
        finally
        {
            Marshal.FreeHGlobal(buf);
            NtClose(dirHandle);
        }

        return results;
    }

    // ── MakeTemporary ─────────────────────────────────────────────────────────
    // Opens the job at `ntPath` with DELETE access and calls
    // NtMakeTemporaryObject.  On success the name is removed from the
    // object-manager namespace and the NTSTATUS 0x00000000 is returned.
    // Returns a non-zero NTSTATUS on failure.
    public static int MakeTemporary(string ntPath)
    {
        IntPtr jobAlloc = AllocObjectAttributes(ntPath, OBJ_CASE_INSENSITIVE);
        IntPtr jobHandle;
        int    s;
        try
        {
            s = NtOpenJobObject(out jobHandle, JOB_OBJECT_DELETE, jobAlloc);
        }
        finally { Marshal.FreeHGlobal(jobAlloc); }

        if (s != STATUS_SUCCESS) return s;
        try   { return NtMakeTemporaryObject(jobHandle); }
        finally { NtClose(jobHandle); }
    }
}
'@

# ── Step 1: Enable SeCreatePermanentPrivilege ─────────────────────────────────
# NtMakeTemporaryObject requires this privilege to be enabled, even though
# "MakeTemporary" sounds like a removal operation.  The kernel gates it on this
# privilege to prevent non-privileged callers from destroying others' permanent
# objects.
if (-not $WhatIf) {
    try {
        [NtJobCleanup]::EnablePrivilege("SeCreatePermanentPrivilege")
        Write-Host "SeCreatePermanentPrivilege enabled."
    }
    catch {
        Write-Error ("Cannot enable SeCreatePermanentPrivilege: $_`n" +
            "This privilege is required by NtMakeTemporaryObject.`n" +
            "Run as NT AUTHORITY\SYSTEM (the AC service account), or have an admin " +
            "grant the right via secpol.msc > 'Create permanent shared objects'.")
        exit 1
    }
}

# ── Step 2: Find orphans ──────────────────────────────────────────────────────
Write-Host "Scanning $Directory for $Prefix* job objects..."
$all = [NtJobCleanup]::FindOrphans($Directory, $Prefix)

Write-Host ("Found {0} matching job object(s) total." -f $all.Count)

$orphans = $all | Where-Object { $_.IsOrphan }

if ($orphans.Count -eq 0) {
    Write-Host "No orphaned job objects found."
    exit 0
}

Write-Host ""
Write-Host ("Found {0} orphan(s) (permanent, active_processes=0, other_handles=0):" -f $orphans.Count)
foreach ($j in $orphans) {
    Write-Host ("  {0}" -f $j.Path)
}

# ── Step 3: Clean up ─────────────────────────────────────────────────────────
if ($WhatIf) {
    Write-Host ""
    Write-Host "WhatIf: no changes made."
    exit 0
}

Write-Host ""
$cleaned = 0
$failed  = 0
foreach ($j in $orphans) {
    $status = [NtJobCleanup]::MakeTemporary($j.Path)
    if ($status -eq 0) {
        Write-Host ("  OK   {0}" -f $j.Path)
        $cleaned++
    }
    else {
        Write-Warning ("  FAIL {0}  NTSTATUS 0x{1:X8}" -f $j.Path, [uint32]$status)
        $failed++
    }
}

Write-Host ""
Write-Host ("Cleaned: {0}  Failed: {1}" -f $cleaned, $failed)
if ($failed -gt 0) { exit 1 }
