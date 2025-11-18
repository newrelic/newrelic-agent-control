# PowerShell script to install New Relic Agent Control as a Windows Service
# Run this script with Administrator privileges

param(

    [Parameter(Mandatory=$false)]
    [string]$ServiceName = "newrelic-agent-control",

    [Parameter(Mandatory=$false)]
    [switch]$ServiceOverwrite = $false
)


# Check for administrator privileges
$currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "Admin permission is required. Please, open a Windows PowerShell session with administrative rights.";
    exit 1
}

# If the service already exists and overwriting is allowed, the service is stopped and removed.
$existingService = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existingService) {
    if ($ServiceOverwrite -eq $false)
    {
        "service $ServiceName already exists. Use flag '-ServiceOverwrite' to update it"
        exit 1
    }
    Write-Host "Service '$ServiceName' already exists. Stopping and removing..."
    Stop-Service $ServiceName | Out-Null

    $serviceToRemove = Get-WmiObject -Class Win32_Service -Filter "name='$ServiceName'"
    if ($serviceToRemove)
    {
        $serviceToRemove.delete() | Out-Null
    }
}

$versionData = (& ".\newrelic-agent-control.exe" --version) -replace ',.*$', ''
Write-Host "Installing $versionData"

# TODO: Making this configurable requires an update on AC
$acDir = [IO.Path]::Combine($env:ProgramFiles, 'New Relic\newrelic-agent-control')
$acDataDir = [IO.Path]::Combine($env:ProgramData, 'New Relic\newrelic-agent-control')
$acLogsDir = [IO.Path]::Combine($acDataDir, 'logs')
$acLocalConfigDir = [IO.Path]::Combine($acDir, 'local-data\agent-control')

$serviceDisplayName = "New Relic Agent Control"

$acExecPath = [IO.Path]::Combine($acDir, 'newrelic-agent-control.exe')


Function Create-Directory ($dir) {
    if (-Not (Test-Path -Path $dir))
    {
        "  Creating $dir"
        New-Item -ItemType directory -Path $dir | Out-Null
    }
}

Write-Host "Creating directories..."

Create-Directory $acDir
Create-Directory $acDataDir
Create-Directory $acLogsDir
Create-Directory $acLocalConfigDir
#Copy-Item -Path ".\LICENSE.txt" -Destination "$acDir"

Write-Host "Copying New Relic Agent Control program files..."
Copy-Item -Path ".\newrelic-agent-control.exe" -Destination "$acDir"

# Generate configuration
# TODO use newrelic-agent-control-cli.exe generate-config instead
# TODO: should we allow optional custom path for config?
Copy-Item -Path ".\config.yaml" -Destination "$acLocalConfigDir\local_config.yaml"

# Install the service
Write-Host "Installing New Relic Agent Control service..."
New-Service -Name $ServiceName -DisplayName "$serviceDisplayName" -BinaryPathName "$acExecPath" -StartupType Automatic | Out-Null
if ($?)
{
    Start-Service -Name $ServiceName | Out-Null
    Write-Host "Installation completed!"
} else {
    Write-Host "Error creating service $ServiceName"
    exit 1
}
