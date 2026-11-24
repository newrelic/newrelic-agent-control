<#
.SYNOPSIS
    Runs an executable as NT AUTHORITY\SYSTEM and relays its output and exit code.

.DESCRIPTION
    SYSTEM holds privileges, such as SeCreatePermanentPrivilege, that the administrator account of
    GitHub-hosted runners does not have, and it is the account the Agent Control service runs as.
    The executable runs from a one-off scheduled task, which needs no extra tooling. Its output is
    printed once it finishes.
#>
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [string[]]$Arguments = @(),
    [string]$WorkingDirectory = (Get-Location).Path,
    [hashtable]$Environment = @{},
    [int]$TimeoutMinutes = 30
)

$ErrorActionPreference = 'Stop'

$taskName = "run-as-system-$([guid]::NewGuid().ToString('N'))"
$workDir = Join-Path $env:RUNNER_TEMP $taskName
New-Item -ItemType Directory -Force -Path $workDir | Out-Null
$wrapperPath = Join-Path $workDir 'wrapper.ps1'
$logPath = Join-Path $workDir 'output.log'
$exitCodePath = Join-Path $workDir 'exit-code.txt'

# The wrapper runs inside the task: it sets the environment, runs the executable and writes the
# exit code last, which is how this script knows the task is done.
$environmentLines = $Environment.GetEnumerator() | ForEach-Object {
    "`$env:$($_.Key) = '$($_.Value)'"
}
$quotedArguments = ($Arguments | ForEach-Object { "'$_'" }) -join ' '
@(
    "Set-Location '$WorkingDirectory'"
    $environmentLines
    "& '$Executable' $quotedArguments 2>&1 | Out-File -Encoding utf8 -FilePath '$logPath'"
    "`$LASTEXITCODE | Out-File -Encoding ascii -FilePath '$exitCodePath'"
) | Set-Content -Path $wrapperPath

try {
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' `
        -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$wrapperPath`""
    $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount `
        -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes $TimeoutMinutes)
    Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal `
        -Settings $settings | Out-Null
    Start-ScheduledTask -TaskName $taskName

    $deadline = (Get-Date).AddMinutes($TimeoutMinutes)
    while (-not (Test-Path $exitCodePath)) {
        if ((Get-Date) -gt $deadline) {
            throw "$Executable did not finish within $TimeoutMinutes minutes"
        }
        Start-Sleep -Seconds 5
    }

    if (Test-Path $logPath) { Get-Content $logPath }
    $exitCode = [int](Get-Content $exitCodePath -Raw).Trim()
}
finally {
    Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
}

exit $exitCode
