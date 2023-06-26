# Packaging tests

Run packaging tests:

```shell
# Default values
make test/packaging \
  NR_LICENSE_KEY=**************** \
  NR_SUPER_AGENT_VERSION=0.0.4 \
  NR_OTEL_COLLECTOR_MEMORY_LIMIT=100 \
  NR_OTEL_COLLECTOR_OTLP_ENDPOINT=staging-otlp.nr-data.net:4317

# Limit to linux
make test/packaging \
  LIMIT=testing_hosts_linux \
  NR_LICENSE_KEY=**************** \
  NR_SUPER_AGENT_VERSION=0.0.4 \
  NR_OTEL_COLLECTOR_MEMORY_LIMIT=100 \
  NR_OTEL_COLLECTOR_OTLP_ENDPOINT=staging-otlp.nr-data.net:4317
```

## Required parameters

* `NR_LICENSE_KEY`: New Relic license key.
* `NR_SUPER_AGENT_VERSION`: The Super Agent version to be installed.
* `NR_OTEL_COLLECTOR_MEMORY_LIMIT`: Memory limit for the NR Otel Collector.
* `NR_OTEL_COLLECTOR_OTLP_ENDPOINT`: OTLP Endpoint for the NR Otel Collector.

## Optional parameters

* `ANSIBLE_INVENTORY`: Path of the Ansible inventory file (default: inventory.ec2).
* `LIMIT`: Ansible inventory group name to limit the execution for (default: testing_hosts_linux).
* `ANSIBLE_FORKS`: Maximum number of concurrent Ansible forks (default: 5).
