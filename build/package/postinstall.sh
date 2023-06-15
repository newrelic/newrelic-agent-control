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
# NR Meta Agent
######################################################################################
if command -v systemctl >/dev/null 2>&1; then
    systemctl enable nr-meta-agent.service
    if [ -f /etc/nr-meta-agent/config.yaml ]; then
        systemctl start nr-meta-agent.service
    fi
fi