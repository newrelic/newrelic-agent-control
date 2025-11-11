######################################################################################
# This script is thought to be added for execution by the custom agent_type used on sub-agent's
# integration tests. It handles CTRL+BREAK events sent by the agentcontrol when a new config
# is received by the sub-agent's opamp so we don't restart the agent and we don't show an error log.
######################################################################################

try {
    # Sleep for 60 seconds (main execution)
    Start-Sleep -Seconds 60
    # Exit gracefully
    exit 0
}
finally {
    Write-Host "Entered final block, will sleep and exit."
    # Sleep for 60 seconds (main execution)
    Start-Sleep -Seconds 60
    # Exit gracefully
    exit 0
}