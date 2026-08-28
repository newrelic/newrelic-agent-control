#!/bin/sh

######################################################################################
# Delete config and running files
######################################################################################

# $1 = "remove"|"purge" on DEB, "0" on RPM. Skip deletion during upgrades ("upgrade" on DEB, "1" on RPM).
case "$1" in
  remove|purge|0)
    # Outside agent-control's filesystem; deleted on uninstall since a standalone newrelic-infra would recreate it.
    rm -rf /var/run/newrelic-infra
    rm -rf /etc/newrelic-agent-control
    rm -rf /var/lib/newrelic-agent-control
    rm -rf /var/log/newrelic-agent-control
    rm -rf /usr/share/doc/newrelic/newrelic-agent-control/
  ;;
esac
