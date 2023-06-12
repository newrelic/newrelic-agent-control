#!/bin/sh

######################################################################################
# NR Meta Agent
######################################################################################
if command -v systemctl >/dev/null 2>&1; then
    systemctl stop nr-meta-agent.service
    systemctl disable nr-meta-agent.service
fi
