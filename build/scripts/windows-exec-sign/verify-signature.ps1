#!/usr/bin/env pwsh
#
# Verify Windows executable signatures
#
# This script verifies that Windows executables are properly signed
# with valid Authenticode signatures.
#
# Usage:
#   verify-signature.ps1 -Executables <exe1>,<exe2>,...
#
# Example:
#   verify-signature.ps1 -Executables "./artifacts/dist/foo.exe","./artifacts/dist/bar.exe"
#

param(
    [Parameter(Mandatory=$true)]
    [string[]]$Executables
)

Write-Host "Verifying signatures for Windows executables"
Write-Host "=============================================="
Write-Host ""

$allValid = $true

foreach ($exePath in $Executables) {
    $exeName = Split-Path -Leaf $exePath

    Write-Host "Checking: $exeName"
    Write-Host "  Path: $exePath"

    if (-not (Test-Path $exePath)) {
        Write-Host "  ERROR: File not found!" -ForegroundColor Red
        $allValid = $false
        Write-Host ""
        continue
    }

    $signature = Get-AuthenticodeSignature -FilePath $exePath

    Write-Host "  Status: $($signature.Status)"

    if ($signature.SignerCertificate) {
        Write-Host "  Signer: $($signature.SignerCertificate.Subject)"
        Write-Host "  Thumbprint: $($signature.SignerCertificate.Thumbprint)"
    }

    if ($signature.Status -ne 'Valid') {
        Write-Host "  ERROR: Signature is not valid!" -ForegroundColor Red
        if ($signature.StatusMessage) {
            Write-Host "  Reason: $($signature.StatusMessage)" -ForegroundColor Red
        }
        $allValid = $false
    } else {
        Write-Host "  SUCCESS: Signature is valid" -ForegroundColor Green
    }

    Write-Host ""
}

Write-Host "=============================================="
if (-not $allValid) {
    Write-Host "FAILED: One or more executables are missing or have invalid signatures" -ForegroundColor Red
    exit 1
}

Write-Host "SUCCESS: All Windows executables are properly signed" -ForegroundColor Green
exit 0
