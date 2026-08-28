#!/bin/sh

######################################################################################
# Newrelic Agent Control
######################################################################################
# Only stop/disable on a true removal, not on an upgrade - the incoming version's
# postinstall already re-enabled/restarted the service, so disabling here would
# leave it disabled after the upgrade (and after the next reboot).
# RPM %preun: $1=0 means no versions will remain. DEB prerm: $1=remove means the same.
case "$1" in
  remove|purge|0)
    if command -v systemctl >/dev/null 2>&1; then
        systemctl stop newrelic-agent-control.service
        systemctl disable newrelic-agent-control.service
    fi
    ;;
esac
