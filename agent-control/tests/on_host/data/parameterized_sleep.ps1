param(
  [int]$TimeoutSeconds
)

Write-Host "Sleeping for $TimeoutSeconds seconds..."
Start-Sleep -Seconds $TimeoutSeconds