#!/bin/sh

######################################################################################
# Delete config and running files
######################################################################################

# Outside agent-control's filesystem; deleted on uninstall since a standalone newrelic-infra would recreate it.
rm -rf /var/run/newrelic-infra
rm -rf /etc/newrelic-agent-control
rm -rf /var/lib/newrelic-agent-control
