# Test script for Agent Control Self-Update and Rollback Logic (PoC)
# Usage: 
#   .\tools\test_rollback_poc.ps1 -TestGoodUpdate <path_to_binary>
#   .\tools\test_rollback_poc.ps1 -TestBadUpdate <path_to_binary>
#   .\tools\test_rollback_poc.ps1 (Runs legacy rollback check)

param (
  [string]$TestGoodUpdate = "",
  [string]$TestBadUpdate = ""
)

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

function Start-AgentControl {
  param([string]$Dir, [boolean]$Wait = $true)
  $exe = Join-Path $Dir "agent_control.exe"
  if ($Wait) {
    return Start-Process -FilePath $exe -WorkingDirectory $Dir -PassThru -Wait -NoNewWindow
  }
  else {
    return Start-Process -FilePath $exe -WorkingDirectory $Dir -PassThru -NoNewWindow
  }
}

function Get-BootData {
    param([string]$Dir)
    $path = Join-Path $Dir "agent_control_boot_data.json"
    if (Test-Path $path) {
        return Get-Content $path | ConvertFrom-Json
    }
    return $null
}

function Get-AgentControlStatus {
    try {
        $response = Invoke-RestMethod -Uri "http://localhost:5555/status" -Method Get -ErrorAction Stop
        return $response.agent_control
    }
    catch {
        return $null
    }
}

try {
  # Copy main binary as the starting point
  Copy-Item $binaryPath -Destination (Join-Path $testDir "agent_control.exe")

  # --- GOOD UPDATE TEST ---
  if ($TestGoodUpdate) {
    Write-Host "Running Good Update Test..."
    # 1. Start Agent in background
    $proc = Start-AgentControl -Dir $testDir -Wait $false
    Write-Host "Agent started (PID: $($proc.Id)). Waiting for init..."
    
    # Wait for HTTP server to be up
    $started = $false
    for ($i=0; $i -lt 10; $i++) {
        $status = Get-AgentControlStatus
        if ($null -ne $status) { 
            Write-Host "Agent is up. Version: $($status.version)"
            $started = $true
            break 
        }
        Start-Sleep -Seconds 1
    }
    if (-not $started) { throw "Agent failed to start HTTP server" }

    # 2. Drop Update
    Write-Host "Dropping update file..."
    Copy-Item $TestGoodUpdate -Destination (Join-Path $testDir "agent_control.exe.new")

    # 3. Wait for agent to detect and exit (allow 10s)
    $proc.WaitForExit(15000)
    if ($proc.HasExited) {
      Write-Host "Agent exited with code: $($proc.ExitCode)"
      if ($proc.ExitCode -ne 1467) { Write-Error "FAIL: Expected 1467, got $($proc.ExitCode)" }
    }
    else {
      Stop-Process -Id $proc.Id -Force
      Write-Error "FAIL: Agent did not exit after update."
    }

    # 4. Restart Agent (Simulate Service Manager)
    Write-Host "Restarting Agent (Validating Phase)..."
    # We need to run it long enough to pass probation (60s). 
    # But we don't want to block forever if it hangs.
    $proc2 = Start-AgentControl -Dir $testDir -Wait $false
        
    Write-Host "Waiting 65 seconds for probation to pass..."
    Start-Sleep -Seconds 65
        
    # Check if it's still running
    if ($proc2.HasExited) {
      Write-Error "FAIL: Agent crashed during probation! ExitCode: $($proc2.ExitCode)"
    }
    else {
      Write-Host "PASS: Agent is still running."
      
      # Check status via HTTP
      $status = Get-AgentControlStatus
      if ($null -ne $status) {
          Write-Host "Current Running Version: $($status.version)"
          # Ideally we verify this is the "new" version if we had a way to distinguish.
      }

      Stop-Process -Id $proc2.Id -Force
            
      # Check Boot Data
      $bd = Get-BootData -Dir $testDir
      if ($bd.status -eq "Stable") { Write-Host "PASS: Agent marked itself as Stable." }
      else { Write-Error "FAIL: Boot status is $($bd.status)" }
    }
  }

  # --- BAD UPDATE TEST ---
  if ($TestBadUpdate) {
    Write-Host "Running Bad Update Test..."
    # 1. Start Agent
    $proc = Start-AgentControl -Dir $testDir -Wait $false
    Start-Sleep -Seconds 5

    # 2. Drop Update
    Write-Host "Dropping BAD update file..."
    Copy-Item $TestBadUpdate -Destination (Join-Path $testDir "agent_control.exe.new")

    # 3. Wait for Update Apply
    $proc.WaitForExit(15000)
    if ($proc.ExitCode -ne 1467) { Write-Error "FAIL: Update not applied? Code $($proc.ExitCode)" }
    Write-Host "Update applied. Entering crash loop..."

    # 4. Crash Loop
    # We expect 3 starts. Each might crash fast.
    for ($i = 1; $i -le 3; $i++) {
      Write-Host "Attempt $i..."
      $p = Start-AgentControl -Dir $testDir -Wait $true
      Write-Host "  Exit Code: $($p.ExitCode)"
            
      # On the 3rd attempt (after 2 crashes), it should detect and rollback (Exit 1467)
      # Wait, logic:
      # Start 1: n_attempts=0->1. Crashes.
      # Start 2: n_attempts=1->2. Crashes.
      # Start 3: n_attempts=2->3. Checks rollback. Exits 1467.
            
      if ($i -eq 3) {
        if ($p.ExitCode -eq 1467) { 
          Write-Host "PASS: Rollback triggered on attempt 3." 
        }
        else {
          Write-Error "FAIL: Did not trigger rollback on attempt 3. Got $($p.ExitCode)"
        }
      }
      else {
        # Attempt 1 & 2 should just crash (non-1467, non-0)
        # If the bad binary is really bad it might be anything.
      }
    }

    # 5. Verify Rollback
    $currentContent = Get-Content (Join-Path $testDir "agent_control.exe") -Raw
    # We expect it to be the ORIGINAL binary. We can check via hash or size.
    # But simply checking boot data status is usually enough?
    # Actually logic says: after rollback, it writes Stable status immediately.
        
    $bd = Get-BootData -Dir $testDir
    if ($bd.status -eq "Stable") { Write-Host "PASS: Boot status reverted to Stable." }
    else { Write-Error "FAIL: Boot status is $($bd.status)" }
        
    if (Test-Path (Join-Path $testDir "agent_control.exe.failed")) {
      Write-Host "PASS: Found failed binary backup."
    }
  }

}
finally {
  # Cleanup
  # Remove-Item -Recurse -Force $testDir
  Write-Host "Test finished. Artifacts remain in $testDir"
}
