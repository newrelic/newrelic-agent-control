#!/bin/sh

######################################################################################
# Infra Agent
# C&P https://github.com/newrelic/infrastructure-agent/blob/master/build/package/after-remove.sh
######################################################################################

runDir=/var/run/newrelic-infra
installDir=/var/db/newrelic-infra
logDir=/var/log/newrelic-infra
configDir=/etc/newrelic-infra

case "$1" in
  purge)
    # dpkg does not remove non empty directories
    rm -rf ${runDir}
    rm -rf ${installDir}
    rm -rf ${logDir}
    rm -rf ${configDir}
  ;;
  *)
  ;;
esac
