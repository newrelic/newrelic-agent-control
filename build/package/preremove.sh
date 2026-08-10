#!/bin/sh

######################################################################################
# Newrelic Agent Control
######################################################################################
if command -v systemctl >/dev/null 2>&1; then
    # Only on real removal, not on upgrade (rpm: "0", dpkg: "remove").
    case "$1" in
        0|remove)
            systemctl stop newrelic-agent-control.service
            systemctl disable newrelic-agent-control.service
            ;;
    esac
fi
