# PowerShell script to install New Relic Agent Control as a Windows Service
# Run this script with Administrator privileges

param(
    [Parameter(Mandatory=$false)]
    [switch]$ServiceOverwrite = $false,

    # Configuration generation inputs
    [Parameter(Mandatory=$false)]
    [switch]$FleetEnabled = $false,

    [Parameter(Mandatory=$false)]
    [string]$Region = "us",

    [Parameter(Mandatory=$false)]
    [string]$AgentSet = "no-agents",

    # Fleet-enabled auth and org parameters (used only when -FleetEnabled is set)
    [Parameter(Mandatory=$false)]
    [string]$FleetId,

    [Parameter(Mandatory=$false)]
    [string]$OrganizationId,

    [Parameter(Mandatory=$false)]
    [string]$AuthParentToken,

    [Parameter(Mandatory=$false)]
    [string]$AuthParentClientId,

    [Parameter(Mandatory=$false)]
    [string]$AuthParentClientSecret,

    [Parameter(Mandatory=$false)]
    [string]$AuthPrivateKeyPath,

    [Parameter(Mandatory=$false)]
    [string]$AuthClientId
)

# Check for administrator privileges
$currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "Admin permission is required. Please, open a Windows PowerShell session with administrative rights.";
    exit 1
}

$serviceName = "newrelic-agent-control"
$serviceDisplayName = "New Relic Agent Control"

$acDir = [IO.Path]::Combine($env:ProgramFiles, 'New Relic\newrelic-agent-control')
$acLocalConfigDir = [IO.Path]::Combine($acDir, 'local-data\agent-control')
$acExecPath = [IO.Path]::Combine($acDir, 'newrelic-agent-control.exe')

$acDataDir = [IO.Path]::Combine($env:ProgramData, 'New Relic\newrelic-agent-control')
$acLogsDir = [IO.Path]::Combine($acDataDir, 'logs')

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

[System.IO.Directory]::CreateDirectory("$acDir") | Out-Null
[System.IO.Directory]::CreateDirectory("$acDataDir") | Out-Null
[System.IO.Directory]::CreateDirectory("$acLogsDir") | Out-Null
[System.IO.Directory]::CreateDirectory("$acLocalConfigDir") | Out-Null

Write-Host "Copying New Relic Agent Control program files..."
Copy-Item -Path ".\newrelic-agent-control.exe" -Destination "$acDir"

# Generate configuration based on inputs
$localConfigPath = Join-Path $acLocalConfigDir 'local_config.yaml'

# Common args
$cliArgs = @(
    'generate-config',
    '--output-path', $localConfigPath,
    '--region', $Region,
    '--agent-set', $AgentSet
)

if (-not $FleetEnabled) {
    $cliArgs += '--fleet-disabled'
} else {
    # If no private key path is provided, a new key will be created in the keys directory
    $createdPrivateKey = $false
    if ([string]::IsNullOrWhiteSpace($AuthPrivateKeyPath)) {
        $keysDir = Join-Path $acDir 'keys'
        [System.IO.Directory]::CreateDirectory($keysDir) | Out-Null
        $AuthPrivateKeyPath = Join-Path $keysDir 'agent-control-identity.key'
        $createdPrivateKey = $true
    }

    if ($FleetId) { $cliArgs += @('--fleet-id', $FleetId) }
    if ($OrganizationId) { $cliArgs += @('--organization-id', $OrganizationId) }
    if ($AuthParentToken) { $cliArgs += @('--auth-parent-token', $AuthParentToken) }
    if ($AuthParentClientId) { $cliArgs += @('--auth-parent-client-id', $AuthParentClientId) }
    if ($AuthParentClientSecret) { $cliArgs += @('--auth-parent-client-secret', $AuthParentClientSecret) }
    if ($AuthPrivateKeyPath) { $cliArgs += @('--auth-private-key-path', $AuthPrivateKeyPath) }
    if ($AuthClientId) { $cliArgs += @('--auth-client-id', $AuthClientId) }
}

Write-Host "Generating configuration with: $($cliArgs -join ' ')"
& ".\newrelic-agent-control-cli.exe" @cliArgs
if ($LASTEXITCODE -ne 0) {
    Write-Error "Configuration generation failed with exit code $LASTEXITCODE"
    exit $LASTEXITCODE
}


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
