# PowerShell script that ignores CTRL+BREAK events (similar to ignoring SIGTERM on Unix)

Write-Host "Script started. PID: $PID"
Write-Host "Waiting forever. Send CTRL+BREAK to see it ignored."
Write-Host "Use Task Manager or Stop-Process -Id $PID -Force to stop forcefully."

# Infinite loop with small sleeps, ignoring attempts to interrupt it
try {
    while ($true) {
        Start-Sleep -Seconds 5
    }
}
finally {
    Write-Host "Entered final block, will continue running."
    while ($true) {
        Start-Sleep -Seconds 5
    }
}
