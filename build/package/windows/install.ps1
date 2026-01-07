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
    [string]$LicenseKey,

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
    [string]$AuthClientId,

    # Proxy configuration (optional)
    [Parameter(Mandatory=$false)]
    [string]$ProxyUrl,

    [Parameter(Mandatory=$false)]
    [string]$ProxyCABundleFile,

    [Parameter(Mandatory=$false)]
    [string]$ProxyCABundleDir,

    [Parameter(Mandatory=$false)]
    [switch]$ProxyIgnoreSystem = $false
)

function Set-RestrictedAcl {
    param (
        [string]$Path
    )
    if ([string]::IsNullOrWhiteSpace($Path)) { return }
    if (-not (Test-Path -Path $Path)) { return }
    # Remove inheritance and replace explicit grants with Administrators members only
    # Giving Full Control Access (F) and making it apply to all child objects (OI)(CI)
    #
    # Use SID instead of group name to avoid localization issues. The group name changes
    # based on the configured language.
    # For example, BUILTIN\Administrators (english) vs BUILTIN\Administradores (spanish).
    # 
    # S-1-5-32-544 is the well-known SID for BUILTIN\Administrators.
    # Reference: https://learn.microsoft.com/es-es/windows-server/identity/ad-ds/manage/understand-security-identifiers
    & icacls $Path /inheritance:r | Out-Null
    & icacls $Path /grant "*S-1-5-32-544:(OI)(CI)F" | Out-Null 
}

# Check for administrator privileges
$currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "Admin permission is required. Please, open a Windows PowerShell session with administrative rights.";
    exit 1
}

$serviceName = "newrelic-agent-control"
$serviceDisplayName = "New Relic Agent Control"

$acProgramFilesDir = [IO.Path]::Combine($env:ProgramFiles, 'New Relic\newrelic-agent-control')
$acLocalConfigDir = [IO.Path]::Combine($acProgramFilesDir, 'local-data\agent-control')
$acIdentityKeyDir = [IO.Path]::Combine($acProgramFilesDir, 'keys')
$acIdentityKeyPath = [IO.Path]::Combine($acIdentityKeyDir, 'agent-control-identity.key')
$acEnvFilePath = [IO.Path]::Combine($acProgramFilesDir, 'environment_variables.yaml')
$acExecPath = [IO.Path]::Combine($acProgramFilesDir, 'newrelic-agent-control.exe')

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

[System.IO.Directory]::CreateDirectory("$acProgramFilesDir") | Out-Null
[System.IO.Directory]::CreateDirectory("$acDataDir") | Out-Null
Set-RestrictedAcl -Path $acProgramFilesDir
Set-RestrictedAcl -Path $acDataDir
[System.IO.Directory]::CreateDirectory("$acLogsDir") | Out-Null
[System.IO.Directory]::CreateDirectory("$acLocalConfigDir") | Out-Null

Write-Host "Copying New Relic Agent Control program files..."
Copy-Item -Path ".\newrelic-agent-control.exe" -Destination "$acProgramFilesDir"

# Generate configuration based on inputs
$localConfigPath = Join-Path $acLocalConfigDir 'local_config.yaml'

# Common args
$cliArgs = @(
    'generate-config',
    '--output-path', $localConfigPath,
    '--region', $Region,
    '--agent-set', $AgentSet,
    '--env-vars-file-path', $acEnvFilePath
)

if ($LicenseKey) { $cliArgs += @('--newrelic-license-key', $LicenseKey)}

# Private key path is provided whenever an existing key is to be used.
# If not provided, a new key will be generated and stored in the keys directory.
if ([string]::IsNullOrWhiteSpace($AuthPrivateKeyPath)) {
    [System.IO.Directory]::CreateDirectory($acIdentityKeyDir) | Out-Null
    $AuthPrivateKeyPath = $acIdentityKeyPath
}

if (-not $FleetEnabled) {
    $cliArgs += '--fleet-disabled'
} else {
    if ($FleetId) { $cliArgs += @('--fleet-id', $FleetId) }
    if ($OrganizationId) { $cliArgs += @('--organization-id', $OrganizationId) }
    if ($AuthParentToken) { $cliArgs += @('--auth-parent-token', $AuthParentToken) }
    if ($AuthParentClientId) { $cliArgs += @('--auth-parent-client-id', $AuthParentClientId) }
    if ($AuthParentClientSecret) { $cliArgs += @('--auth-parent-client-secret', $AuthParentClientSecret) }
    if ($AuthPrivateKeyPath) { $cliArgs += @('--auth-private-key-path', $AuthPrivateKeyPath) }
    if ($AuthClientId) { $cliArgs += @('--auth-client-id', $AuthClientId) }
}

# Proxy args (optional)
if ($ProxyUrl) { $cliArgs += @('--proxy-url', $ProxyUrl)}
if ($ProxyCABundleFile) { $cliArgs += @('--proxy-ca-bundle-file', $ProxyCABundleFile) }
if ($ProxyCABundleDir) { $cliArgs += @('--proxy-ca-bundle-dir', $ProxyCABundleDir) }
if ($ProxyIgnoreSystem) { $cliArgs += '--ignore-system-proxy', 'true' }

Write-Host "Generating configuration..."
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

# Verify service is running and AC is healthy
$MAX_RETRIES = 10
$TRIES = 0

while ($TRIES -lt $MAX_RETRIES) {
    $TRIES++
    Write-Host "Running agent status check attempt $TRIES/$MAX_RETRIES..."

    Try { $statusCheckOutput = Invoke-WebRequest -Uri "http://localhost:51200/status" -UseBasicParsing -ErrorAction SilentlyContinue } Catch {    }
    $statusContent = if ($statusCheckOutput.Content) { $statusCheckOutput.Content } else { "{}" }

    # Parse JSON for .agent_control.healthy
    $statusJson = $statusContent | ConvertFrom-Json
    $STATUS = $statusJson.agent_control.healthy

    if ($STATUS -eq $true) {
        Write-Host "Agent status check ok."
        break
    } elseif ($TRIES -eq $MAX_RETRIES) {
        Write-Error "New Relic Agent Control has not started or is un-healthy after installing. Please try again later, or see our documentation for installing manually https://docs.newrelic.com/docs/using-new-relic/cross-product-functions/install-configure/install-new-relic"
        exit 31
    }
    Start-Sleep -Seconds 30
}
