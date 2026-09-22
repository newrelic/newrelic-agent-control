#!/bin/sh

######################################################################################
# Delete config and running files
######################################################################################

# Only wipe config/data/logs on full purge (DEB) or RPM uninstall.
# On DEB 'remove', preserve these so dpkg conffile state stays consistent and reinstall works correctly.
case "$1" in
  purge|0)
    # Outside agent-control's filesystem; deleted on uninstall since a standalone newrelic-infra would recreate it.
    rm -rf /var/run/newrelic-infra
    rm -rf /etc/newrelic-agent-control
    rm -rf /var/lib/newrelic-agent-control
    rm -rf /var/log/newrelic-agent-control
    rm -rf /usr/share/doc/newrelic/newrelic-agent-control/
  ;;
esac
