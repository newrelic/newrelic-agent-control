@{
    ExcludeRules = @(
        # Our scripts are standalone installers/CI tooling run non-interactively, not reusable
        # modules exposing cmdlets, so console output via Write-Host is the intended behavior,
        # not an anti-pattern.
        'PSAvoidUsingWriteHost',

        # Only relevant for public cmdlets that need -WhatIf/-Confirm support; none of our
        # scripts expose cmdlets for interactive/confirmable use.
        'PSUseShouldProcessForStateChangingFunctions',

        # Get-WmiObject is used by build/package/windows/{un,}install.ps1 to delete the Windows
        # service during reinstall/uninstall. These target Windows PowerShell 5.1 (not pwsh),
        # where WMI cmdlets are available and well-tested in production; swapping to
        # Get-CimInstance/Invoke-CimMethod needs verification on an actual Windows host, which
        # isn't available in this change.
        'PSAvoidUsingWMICmdlet',

        # build/package/windows/install.ps1's -ServiceOverwrite switch intentionally defaults to
        # $true to preserve existing installer behavior (overwrite-by-default); flipping the
        # default would be a breaking behavior change, not a lint fix.
        'PSAvoidDefaultValueSwitchParameter'
    )
}
