# PowerShell script to uninstall the New Relic Agent Control Windows Service
# Run this script with Administrator privileges

# Check for administrator privileges
$currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "Admin permission is required. Please, open a Windows PowerShell session with administrative rights.";
    exit 1
}

$serviceName = "newrelic-agent-control"
$acDir = [IO.Path]::Combine($env:ProgramFiles, 'New Relic\newrelic-agent-control')


# Stop and remove the service if exists
$existingService = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
if ($existingService) {
    Write-Host "Stopping and removing $serviceName..."
    Stop-Service $serviceName | Out-Null

    $serviceToRemove = Get-WmiObject -Class Win32_Service -Filter "name='$serviceName'"
    if ($serviceToRemove)
    {
        $serviceToRemove.delete() | Out-Null
    }
}

# Remove ProgramData runtime directory (logs, rendered sub-agent configs, local-data/).
# These are created at runtime and are not part of the installer, so the steps above never touch them.
$programDataDir = Join-Path $env:ProgramData "New Relic\newrelic-agent-control"
if (Test-Path $programDataDir) {
    try {
        Remove-Item -Path $programDataDir -Recurse -Force -ErrorAction Stop
    } catch {
        throw "Could not remove '$programDataDir': $_. Close any Explorer windows or applications that have it open, then re-run this script."
    }
}

# Remove ProgramFiles install directory last (exe, keys/, install marker, and this script).
# Also acts as a fallback if the service removal above was skipped.
# Delete all contents except this script first so that if the final directory removal fails,
# the script is still present and re-runnable.
Get-ChildItem -Path $acDir -Force -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -ne "uninstall.ps1" } |
    ForEach-Object { Remove-Item -Path $_.FullName -Recurse -Force -ErrorAction SilentlyContinue }
if (Test-Path $acDir) {
    try {
        Remove-Item -Path $acDir -Recurse -Force -ErrorAction Stop
    } catch {
        throw "Could not remove '$acDir': $_. Close any Explorer windows or applications that have it open, then re-run this script."
    }
}

Write-Host "New Relic Agent Control has been removed from this host."
