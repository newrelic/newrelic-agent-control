#!/bin/sh

######################################################################################
# This script is thought to be added for execution by the custom agent_type used on sub-agent's
# integration tests, it traps the sigterm sent by the agentcontrol when a new config is received
# by the sub-agent's opamp so we don't restart the agent and we don't show an error log.
######################################################################################

trap "sleep 60;exit 0" TERM; sleep 60
