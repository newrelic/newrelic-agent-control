# Test script for Agent Control Rollback Logic (PoC)
# Usage: .\tools\test_rollback_poc.ps1

$ErrorActionPreference = "Stop"

$workspaceRoot = Get-Location
$binaryPath = Join-Path $workspaceRoot "target\debug\newrelic-agent-control.exe"

if (-not (Test-Path $binaryPath)) {
  Write-Error "Binary not found at $binaryPath. Please run 'cargo build --bin newrelic-agent-control' first."
  exit 1
}

# Create a temporary directory for the test
$testDir = Join-Path ([System.IO.Path]::GetTempPath()) "ac_test_rollback_$(Get-Random)"
New-Item -ItemType Directory -Force -Path $testDir | Out-Null
Write-Host "Test running in: $testDir"

try {
  # 1. Setup Files
  # Current version (The "Broken" one)
  Copy-Item $binaryPath -Destination (Join-Path $testDir "agent_control.exe")
    
  # Backup version (The "Stable" one) - Just a dummy file for now, 
  # but we give it .exe extension so the rename logic works.
  # In a real scenario, this would be a valid executable.
  $backupContent = "I am the stable backup version"
  $backupPath = Join-Path $testDir "agent_control.exe.old"
  Set-Content -Path $backupPath -Value $backupContent

  # 2. Setup Boot Data (Simulate 2 crashes already)
  # We set status to "Validating" and n_attempts to 2.
  # The next run should increment to 3 and trigger rollback.
  $currentTimestamp = [int][double]::Parse((Get-Date -UFormat %s))
  $bootData = @{
    status               = "Validating"
    current_version      = "1.0.0" # Fixed version so it doesn't reset on mismatch
    n_attempts           = 2
    backup_path          = "agent_control.exe.old"
    last_crash_timestamp = $currentTimestamp
  }
  # Note: CARGO_PKG_VERSION is built into the binary. 
  # To test properly, logic in main_onhost.rs compares persisted `current_version` with env!("CARGO_PKG_VERSION").
  # If they differ, it resets.
  # So we need to ensure the `current_version` in json matches the binary version, OR we trust the "reset on mismatch" logic
  # but that would reset attempts to 1, failing the test.
  # We need to know the version of the built binary.
    
  # Let's extract version from Cargo.toml to be safe, or just let the reset happen and force 3 runs loop in this script?
  # No, let's try to pass the matching version.
    
  # Easier hack: We let the first run happen, it might reset to 1 if version mismatch.
  # But if we pre-calculate "3" attempts, it rolls back immediately.
  # Wait, if version mismatches, code does: n_attempts=1, status=Validating.
  # We want it to think it IS the current version.
  # We can run the binary with `--version` first to grab it?
  $versionOutput = & (Join-Path $testDir "agent_control.exe") --version
  # Output format: "newrelic-agent-control 1.0.0"
  $version = ($versionOutput -split ' ')[1]
  Write-Host "Detected binary version: $version"
  $bootData.current_version = $version
    
  $bootDataJson = $bootData | ConvertTo-Json
  Set-Content -Path (Join-Path $testDir "agent_control_boot_data.json") -Value $bootDataJson

  Write-Host "Initial state prepared. Running agent..."

  # 3. Run the Agent
  # We expect it to exit with code 1467 (ERROR_RESTART_APPLICATION)
  $process = Start-Process -FilePath (Join-Path $testDir "agent_control.exe") -WorkingDirectory $testDir -PassThru -Wait -NoNewWindow
    
  # 4. Assertions
  $exitCode = $process.ExitCode
  Write-Host "Agent exited with code: $exitCode"

  if ($exitCode -ne 1467) {
    Write-Error "FAIL: Expected exit code 1467, got $exitCode"
  }
  else {
    Write-Host "PASS: Exit code matches rollback signal."
  }

  # Verify File Swaps
  if (Test-Path (Join-Path $testDir "agent_control.exe.failed")) {
    Write-Host "PASS: Failed executable was renamed to .failed"
  }
  else {
    Write-Error "FAIL: agent_control.exe.failed not found"
  }

  # Verify the current 'agent_control.exe' is now the backup
  $newContent = Get-Content (Join-Path $testDir "agent_control.exe") -Raw
  if ($newContent -match "I am the stable backup version") {
    Write-Host "PASS: Current executable is now the backup version."
  }
  else {
    Write-Error "FAIL: Current executable content does not match backup."
  }

  # Verify Boot Data is reset to Stable
  $newBootData = Get-Content (Join-Path $testDir "agent_control_boot_data.json") | ConvertFrom-Json
  if ($newBootData.status -eq "Stable") {
    Write-Host "PASS: Boot status marked as Stable."
  }
  else {
    Write-Error "FAIL: Boot status is $($newBootData.status), expected Stable."
  }

}
finally {
  # Cleanup
  # Remove-Item -Recurse -Force $testDir
  Write-Host "Test finished. Artifacts remain in $testDir"
}
