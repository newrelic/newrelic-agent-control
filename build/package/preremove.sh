#!/bin/sh

######################################################################################
# Newrelic Agent Control
######################################################################################
if command -v systemctl >/dev/null 2>&1; then
    systemctl stop newrelic-agent-control.service
    systemctl disable newrelic-agent-control.service
fi
