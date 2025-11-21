# Execute newrelic-agent-control.exe --version and verify output
$output = & .\newrelic-agent-control.exe --version 2>&1

if ($output -match "New Relic Agent Control Version") {
    Write-Host "Version check passed: Found 'New Relic Agent Control Version' in output" -ForegroundColor Green
    Write-Host "Output: $output"
    exit 0
} else {
    Write-Host "Version check failed: 'New Relic Agent Control Version' not found in output" -ForegroundColor Red
    Write-Host "Output: $output"
    exit 1
}
