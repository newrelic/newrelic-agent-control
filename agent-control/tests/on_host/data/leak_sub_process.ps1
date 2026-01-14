param(
  # Just a unique identifier to make simple to find if running from tests
  # with a command like: `gwmi Win32_Process | ? CommandLine -match "$Id" | select ProcessId, CommandLine`
  [string]$Id = "default"

)

# Windows-only simple sub-process that sleeps
$proc = Start-Process -FilePath 'powershell.exe' -ArgumentList @(
  '-NoProfile'
  '-NonInteractive'
  '-Command'
  "# test-id: $Id
   Start-Sleep -Seconds 30"
) -PassThru -WindowStyle Hidden
