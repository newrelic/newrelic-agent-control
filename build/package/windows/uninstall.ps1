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

# Remove ProgramFiles install directory (exe, keys/, install marker, and this script).
# Also acts as a fallback if the service removal above was skipped.
Remove-Item -Path $acDir -Recurse -Force -ErrorAction SilentlyContinue

# Remove ProgramData runtime directory (logs, rendered sub-agent configs, local-data/).
# These are created at runtime and are not part of the installer, so the steps above never touch them.
Remove-Item -Path (Join-Path $env:ProgramData "New Relic\newrelic-agent-control") -Recurse -Force -ErrorAction SilentlyContinue

Write-Host "New Relic Agent Control has been removed from this host."
