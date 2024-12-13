#!/bin/sh

######################################################################################
# Infra Agent
# C&P https://github.com/newrelic/infrastructure-agent/blob/master/build/package/rpm/postinst-systemd.sh
######################################################################################

oldPid=/var/run/newrelic-infra.pid
# Previous versions of the agent didn't remove the pid, it's removed manually
# here because current versions of the agent use a different location.
if [ -e "$oldPid" ]; then
  rm "$oldPid"
fi

######################################################################################
# Newrelic Agent Control
######################################################################################
if command -v systemctl >/dev/null 2>&1; then
    systemctl enable newrelic-agent-control.service
    if [ -f /etc/newrelic-agent-control/config.yaml ]; then
        systemctl start newrelic-agent-control.service
    fi
fi