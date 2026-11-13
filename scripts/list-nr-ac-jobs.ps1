#Requires -Version 5.1
<#
.SYNOPSIS
    Lists named Job Objects in \BaseNamedObjects whose name begins with "nr-ac-".

.DESCRIPTION
    For each matching job this script reports:
      - Full NT object-manager path
      - Whether the object is permanent (survives all handle closes until
        NtMakeTemporaryObject is called)
      - How many handles are currently open to it, EXCLUDING this script's
        own query handle (so 0 means nothing else holds it open)
      - How many processes are currently assigned to it
      - The PIDs of those processes

    The script calls NT and Win32 APIs directly via P/Invoke and requires
    no external tools or binaries.

    Run as NT AUTHORITY\SYSTEM (the account Agent Control uses as a Windows
    service), or as any account that has JOB_OBJECT_QUERY access on the job
    objects (determined by each object's DACL).

.PARAMETER Prefix
    Only list job objects whose name starts with this string.
    Default: "nr-ac-"

.PARAMETER Directory
    NT object-manager path of the directory to enumerate.
    Default: "\BaseNamedObjects"  (the global namespace, reachable from all
    sessions including Session 0 where AC runs as a service)

.EXAMPLE
    # List all nr-ac-* jobs
    .\list-nr-ac-jobs.ps1

.EXAMPLE
    # Restrict to a specific agent type prefix
    .\list-nr-ac-jobs.ps1 -Prefix "nr-ac-infrastructure-agent"
#>
param(
    [string] $Prefix    = "nr-ac-",
    [string] $Directory = "\BaseNamedObjects"
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

public static class NtJobList
{
    // ── Access rights ─────────────────────────────────────────────────────────
    public const uint DIRECTORY_QUERY  = 0x00000001;
    public const uint JOB_OBJECT_QUERY = 0x00000004;

    // ── Object-attribute flags ────────────────────────────────────────────────
    public const uint OBJ_CASE_INSENSITIVE = 0x00000040;
    public const uint OBJ_PERMANENT        = 0x00000010;

    // ── NTSTATUS codes ────────────────────────────────────────────────────────
    const int STATUS_SUCCESS         = 0x00000000;
    const int STATUS_MORE_ENTRIES    = 0x00000105;
    const int STATUS_NO_MORE_ENTRIES = unchecked((int)0x8000001A);

    // ── Job-object information classes ────────────────────────────────────────
    // JobObjectBasicAccountingInformation (1): active-process count and totals
    // JobObjectBasicProcessIdList         (3): array of member PIDs
    const int JobObjectBasicAccountingInformation = 1;
    const int JobObjectBasicProcessIdList         = 3;

    // ── NtQueryObject information class ──────────────────────────────────────
    // ObjectBasicInformation (0): returns OBJECT_BASIC_INFORMATION, which
    // includes Attributes (contains OBJ_PERMANENT flag) and HandleCount.
    const int ObjectBasicInformation = 0;

    // ── Struct sizes (x64 fixed) ──────────────────────────────────────────────
    const int UNICODE_STRING_SIZE    = 16;   // 2+2+4pad+8
    const int OBJECT_ATTRIBUTES_SIZE = 48;   // 4+4pad+8+8+4+4pad+8+8

    // ── OBJECT_BASIC_INFORMATION ──────────────────────────────────────────────
    // Returned by NtQueryObject(handle, ObjectBasicInformation, ...).
    // Attributes carries OBJ_PERMANENT (bit 4) when the object is permanent.
    // HandleCount is the total number of open handles including our own.
    [StructLayout(LayoutKind.Sequential)]
    struct OBJECT_BASIC_INFORMATION
    {
        public uint Attributes;
        public uint GrantedAccess;
        public uint HandleCount;     // includes this script's query handle
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
    // Returned by QueryInformationJobObject(hJob, 1, ...).
    // ActiveProcesses is the current live member count.
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

    // ── P/Invoke declarations ─────────────────────────────────────────────────

    // Opens a directory object by NT path (e.g. "\BaseNamedObjects").
    [DllImport("ntdll.dll")]
    static extern int NtOpenDirectoryObject(
        out IntPtr DirectoryHandle,
        uint       DesiredAccess,
        IntPtr     ObjectAttributes);   // pointer to OBJECT_ATTRIBUTES

    // Enumerates entries in an object-manager directory.
    // Each entry in the output buffer is an OBJECT_DIRECTORY_INFORMATION:
    //   two consecutive UNICODE_STRINGs (Name, TypeName), 32 bytes each on x64.
    // A zeroed entry (both Length == 0) marks the end of the list.
    [DllImport("ntdll.dll")]
    static extern int NtQueryDirectoryObject(
        IntPtr   DirectoryHandle,
        IntPtr   Buffer,
        int      Length,
        byte     ReturnSingleEntry,   // BOOLEAN (1 byte): 0 = return all entries
        byte     RestartScan,         // BOOLEAN (1 byte): 1 = start from beginning
        ref uint Context,             // opaque cursor, updated on each call
        out uint ReturnLength);

    // Opens a named job object by NT path.
    [DllImport("ntdll.dll")]
    static extern int NtOpenJobObject(
        out IntPtr JobHandle,
        uint       DesiredAccess,
        IntPtr     ObjectAttributes);  // pointer to OBJECT_ATTRIBUTES

    // Queries metadata about any kernel object.
    [DllImport("ntdll.dll")]
    static extern int NtQueryObject(
        IntPtr Handle,
        int    ObjectInformationClass,
        IntPtr ObjectInformation,
        int    ObjectInformationLength,
        out int ReturnLength);

    [DllImport("ntdll.dll")]
    static extern int NtClose(IntPtr Handle);

    // Returns job object information. Unlike the Nt* functions, this is a
    // documented Win32 API in kernel32.dll.
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool QueryInformationJobObject(
        IntPtr hJob,
        int    JobObjectInformationClass,
        IntPtr lpJobObjectInfo,
        int    cbJobObjectInfoLength,
        out int lpReturnLength);

    // ── AllocObjectAttributes ─────────────────────────────────────────────────
    // Allocates a contiguous unmanaged block containing:
    //   [OBJECT_ATTRIBUTES (48 bytes)]
    //   [UNICODE_STRING    (16 bytes)]
    //   [UTF-16 path bytes          ]
    // Returns the allocation address, which is also the OBJECT_ATTRIBUTES
    // pointer to pass to NT functions.  Caller must free with
    // Marshal.FreeHGlobal when the NT call has returned.
    static IntPtr AllocObjectAttributes(string ntPath, uint attributes)
    {
        byte[] pathBytes = Encoding.Unicode.GetBytes(ntPath);
        int    total     = OBJECT_ATTRIBUTES_SIZE + UNICODE_STRING_SIZE + pathBytes.Length;
        IntPtr alloc     = Marshal.AllocHGlobal(total);

        // Zero the entire block so padding bytes and null pointers are correct.
        for (int i = 0; i < total; i++) Marshal.WriteByte(alloc, i, 0);

        // Layout within alloc:
        IntPtr oaPtr   = alloc;                                      // OBJECT_ATTRIBUTES
        IntPtr usPtr   = alloc + OBJECT_ATTRIBUTES_SIZE;             // UNICODE_STRING
        IntPtr pathPtr = alloc + OBJECT_ATTRIBUTES_SIZE + UNICODE_STRING_SIZE; // path data

        // Write path data.
        Marshal.Copy(pathBytes, 0, pathPtr, pathBytes.Length);

        // Write UNICODE_STRING fields at usPtr:
        //   +0  Length        (ushort, bytes)
        //   +2  MaximumLength (ushort, bytes)
        //   +4  padding       (4 bytes, already zeroed)
        //   +8  Buffer        (IntPtr, pointer to path data)
        Marshal.WriteInt16(usPtr, 0, (short)pathBytes.Length);
        Marshal.WriteInt16(usPtr, 2, (short)pathBytes.Length);
        Marshal.WriteIntPtr(usPtr, 8, pathPtr);

        // Write OBJECT_ATTRIBUTES fields at oaPtr:
        //   +0  Length                 (int,    must be 48)
        //   +4  padding                (4 bytes, already zeroed)
        //   +8  RootDirectory          (IntPtr, null = absolute path)
        //   +16 ObjectName             (IntPtr, pointer to UNICODE_STRING above)
        //   +24 Attributes             (uint)
        //   +28 padding                (4 bytes, already zeroed)
        //   +32 SecurityDescriptor     (IntPtr, null = default)
        //   +40 SecurityQualityOfService (IntPtr, null)
        Marshal.WriteInt32(oaPtr,   0, OBJECT_ATTRIBUTES_SIZE);
        Marshal.WriteIntPtr(oaPtr, 16, usPtr);
        Marshal.WriteInt32(oaPtr,  24, (int)attributes);

        return alloc; // == oaPtr
    }

    // ── Public result type ────────────────────────────────────────────────────
    public class JobEntry
    {
        public string  Path;
        public bool    Permanent;
        public uint    OtherHandles;    // open handles excluding this script's own
        public uint    ActiveProcesses;
        public ulong[] Pids;
    }

    // ── ListJobs ──────────────────────────────────────────────────────────────
    // Enumerates `directory`, returns all Job objects whose name starts with
    // `prefix` (pass an empty string to return all jobs).
    public static List<JobEntry> ListJobs(string directory, string prefix)
    {
        var results = new List<JobEntry>();

        // ── 1. Open the NT object-manager directory ───────────────────────────
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

        // ── 2. Enumerate entries ──────────────────────────────────────────────
        // 64 KiB is enough to hold all entries in \BaseNamedObjects in one call.
        const int BufSize = 65536;
        IntPtr    buf     = Marshal.AllocHGlobal(BufSize);
        try
        {
            uint ctx     = 0;
            byte restart = 1;  // 1 = restart scan from the beginning

            while (true)
            {
                uint retLen;
                int s = NtQueryDirectoryObject(
                    dirHandle, buf, BufSize,
                    /*ReturnSingleEntry*/ 0,
                    restart,
                    ref ctx, out retLen);
                restart = 0;  // subsequent calls continue from where we left off

                if (s == STATUS_NO_MORE_ENTRIES) break;
                if (s != STATUS_SUCCESS && s != STATUS_MORE_ENTRIES)
                    throw new Exception(
                        string.Format("NtQueryDirectoryObject: NTSTATUS 0x{0:X8}", (uint)s));

                // Parse entries.
                // Each OBJECT_DIRECTORY_INFORMATION is two consecutive
                // UNICODE_STRINGs (Name then TypeName), 32 bytes total on x64.
                // The list is terminated by an entry where both Length fields
                // are zero.
                int offset = 0;
                while (true)
                {
                    // Name.Length at offset+0, TypeName.Length at offset+16
                    ushort nameLen = (ushort)Marshal.ReadInt16(buf, offset + 0);
                    ushort typeLen = (ushort)Marshal.ReadInt16(buf, offset + 16);
                    if (nameLen == 0 && typeLen == 0) break;  // end of list

                    // Name.Buffer at offset+8, TypeName.Buffer at offset+24
                    IntPtr nameBuf = Marshal.ReadIntPtr(buf, offset + 8);
                    IntPtr typeBuf = Marshal.ReadIntPtr(buf, offset + 24);
                    string name     = Marshal.PtrToStringUni(nameBuf, nameLen / 2);
                    string typeName = Marshal.PtrToStringUni(typeBuf, typeLen / 2);
                    offset += 32;  // advance to next entry (32 = 2 × 16-byte UNICODE_STRING)

                    if (!typeName.Equals("Job", StringComparison.OrdinalIgnoreCase))
                        continue;
                    if (prefix.Length > 0 &&
                        !name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                        continue;

                    // ── 3. Open the matching job ──────────────────────────────
                    string fullPath  = directory + "\\" + name;
                    IntPtr jobAlloc  = AllocObjectAttributes(fullPath, OBJ_CASE_INSENSITIVE);
                    IntPtr jobHandle;
                    try
                    {
                        int os = NtOpenJobObject(out jobHandle, JOB_OBJECT_QUERY, jobAlloc);
                        if (os != STATUS_SUCCESS) continue;  // DACL prevents access: skip
                    }
                    finally { Marshal.FreeHGlobal(jobAlloc); }

                    try
                    {
                        var e = new JobEntry { Path = fullPath, Pids = new ulong[0] };

                        // ── 4. Permanence and handle count ────────────────────
                        // NtQueryObject(ObjectBasicInformation) returns
                        // OBJECT_BASIC_INFORMATION.  Attributes bit 0x10 is
                        // OBJ_PERMANENT.  HandleCount includes our own handle
                        // so we subtract 1 to get the "other" count.
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

                        // ── 5. Active-process count ───────────────────────────
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

                        // ── 6. Member PIDs ────────────────────────────────────
                        // JOBOBJECT_BASIC_PROCESS_ID_LIST layout:
                        //   DWORD   NumberOfAssignedProcesses  offset 0
                        //   DWORD   NumberOfProcessIdsInList   offset 4
                        //   ULONG_PTR ProcessIdList[N]         offset 8
                        //                       (8 bytes per entry on x64)
                        const int MaxPids    = 1024;
                        int       pidBufSize = 8 + MaxPids * 8;
                        IntPtr    pidBuf     = Marshal.AllocHGlobal(pidBufSize);
                        try
                        {
                            int retLen2;
                            if (QueryInformationJobObject(jobHandle,
                                    JobObjectBasicProcessIdList,
                                    pidBuf, pidBufSize, out retLen2))
                            {
                                uint count = (uint)Marshal.ReadInt32(pidBuf, 4);
                                e.Pids = new ulong[count];
                                for (int i = 0; i < (int)count; i++)
                                    e.Pids[i] = (ulong)Marshal.ReadInt64(
                                        pidBuf, 8 + i * 8);
                            }
                        }
                        finally { Marshal.FreeHGlobal(pidBuf); }

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
}
'@

# ── Run and display ───────────────────────────────────────────────────────────
$jobs = [NtJobList]::ListJobs($Directory, $Prefix)

if ($jobs.Count -eq 0) {
    Write-Host "No job objects found matching '$Prefix*' in $Directory"
}
else {
    foreach ($j in $jobs) {
        $pidList = if ($j.Pids.Length -gt 0) { $j.Pids -join ', ' } else { '' }
        Write-Host (
            '{0}  permanent={1}  other_handles={2}  active_processes={3}  pids=[{4}]' -f
            $j.Path, $j.Permanent, $j.OtherHandles, $j.ActiveProcesses, $pidList
        )
    }
    Write-Host ''
    Write-Host "$($jobs.Count) job object(s) found."
}
