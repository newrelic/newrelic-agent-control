#!/bin/sh

######################################################################################
# NR Super Agent
######################################################################################
if command -v systemctl >/dev/null 2>&1; then
    systemctl stop newrelic-super-agent.service
    systemctl disable newrelic-super-agent.service
fi
