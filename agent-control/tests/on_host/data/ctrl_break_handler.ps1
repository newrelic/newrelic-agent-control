# Usage: ctrl_break_handler.ps1 <dir> <mode>
#   mode "handle": on CTRL_BREAK write <dir>\got_break and exit 0
#   mode "ignore": on CTRL_BREAK do nothing and keep running
#   mode "ignore_with_child": like "ignore", but first starts a child in the same console and
#     process group running mode "handle" with <dir>\child as its dir, so the child's
#     got_break only appears if CTRL_BREAK reached it directly.
# Writes <dir>\ready once the handler is installed (and, for "ignore_with_child", once the
# child's handler is installed too).
param([string]$Dir, [string]$Mode)

Add-Type -TypeDefinition @"
using System;
using System.IO;
using System.Runtime.InteropServices;
public static class CtrlBreak {
    public delegate bool Handler(uint ctrlType);
    [DllImport("kernel32.dll")]
    public static extern bool SetConsoleCtrlHandler(Handler handler, bool add);
    static Handler keep;
    public static void Install(string dir, bool exitOnBreak) {
        keep = ctrlType => {
            if (ctrlType == 1) { // CTRL_BREAK_EVENT
                if (exitOnBreak) {
                    File.WriteAllText(Path.Combine(dir, "got_break"), "1");
                    Environment.Exit(0);
                }
                return true; // handled: do not terminate
            }
            return false;
        };
        SetConsoleCtrlHandler(keep, true);
    }
}
"@

if ($Mode -eq "ignore_with_child") {
    $ChildDir = Join-Path $Dir "child"
    New-Item -ItemType Directory -Path $ChildDir | Out-Null
    Start-Process -FilePath "powershell" -NoNewWindow -ArgumentList @(
        "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $PSCommandPath, $ChildDir, "handle"
    )
    while (-not (Test-Path (Join-Path $ChildDir "ready"))) { Start-Sleep -Milliseconds 100 }
}

[CtrlBreak]::Install($Dir, $Mode -eq "handle")
Set-Content -Path (Join-Path $Dir "ready") -Value "1"
Start-Sleep -Seconds 120
