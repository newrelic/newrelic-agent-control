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
$acExecPath = [IO.Path]::Combine($acDir, 'newrelic-agent-control.exe')

$markerPath = [IO.Path]::Combine($acDir, '.nr-ac-install')

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

# Remove the executable if exists
if (Test-Path $acExecPath) {
    Write-Host "Deleting $acExecPath..."
    Remove-Item -Path $acExecPath -Force
}

if (Test-Path $markerPath) {
    Write-Host "Removing installation marker file..."
    Remove-Item -Path $markerPath -Force
}

Write-Host "Uninstallation completed!"
