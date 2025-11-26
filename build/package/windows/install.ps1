# PowerShell script to install New Relic Agent Control as a Windows Service
# Run this script with Administrator privileges

param(
    [Parameter(Mandatory=$false)]
    [switch]$ServiceOverwrite = $false
)

# Check for administrator privileges
$currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "Admin permission is required. Please, open a Windows PowerShell session with administrative rights.";
    exit 1
}

$serviceName = "newrelic-agent-control"
$acDir = [IO.Path]::Combine($env:ProgramFiles, 'New Relic\newrelic-agent-control')
$acDataDir = [IO.Path]::Combine($env:ProgramData, 'New Relic\newrelic-agent-control')
$acLogsDir = [IO.Path]::Combine($acDataDir, 'logs')
$acLocalConfigDir = [IO.Path]::Combine($acDir, 'local-data\agent-control')
$serviceDisplayName = "New Relic Agent Control"
$acExecPath = [IO.Path]::Combine($acDir, 'newrelic-agent-control.exe')

# If the service already exists and overwriting is allowed, the service is stopped and removed.
$existingService = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
if ($existingService) {
    if ($ServiceOverwrite -eq $false)
    {
        "service $serviceName already exists. Use flag '-ServiceOverwrite' to update it"
        exit 1
    }
    Write-Host "Service '$serviceName' already exists. Stopping and removing..."
    Stop-Service $serviceName | Out-Null

    $serviceToRemove = Get-WmiObject -Class Win32_Service -Filter "name='$serviceName'"
    if ($serviceToRemove)
    {
        $serviceToRemove.delete() | Out-Null
    }
}

$versionData = (& ".\newrelic-agent-control.exe" --version) -replace ',.*$', ''
Write-Host "Installing $versionData"

Write-Host "Creating New Relic Agent Control directories..."
Function Create-Directory ($dir) {
    if (-Not (Test-Path -Path $dir))
    {
        "  Creating $dir"
        New-Item -ItemType directory -Path $dir | Out-Null
    }
}
Create-Directory $acDir
Create-Directory $acDataDir
Create-Directory $acLogsDir
Create-Directory $acLocalConfigDir

Write-Host "Copying New Relic Agent Control program files..."
Copy-Item -Path ".\newrelic-agent-control.exe" -Destination "$acDir"

# Generate configuration
# TODO: make this configurable through ps1 arguments (identity related args, region, ...)
& ".\newrelic-agent-control-cli.exe" generate-config --fleet-disabled --region us --agent-set no-agents --output-path "`"$acLocalConfigDir\local_config.yaml`""

# Install the service
Write-Host "Installing New Relic Agent Control service..."
New-Service -Name $serviceName -DisplayName "$serviceDisplayName" -BinaryPathName "$acExecPath" -StartupType Automatic | Out-Null
if ($?)
{
    Start-Service -Name $serviceName | Out-Null
    Write-Host "Installation completed!"
} else {
    Write-Host "Error creating service $serviceName"
    exit 1
}
