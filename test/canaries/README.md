# Newrelic Super Agent canaries
This directory contains an Ansible playbook and its dependencies to deploy
Newrelic Super Agent collector containers into a host.

## Usage

Populate the [inventory.yml](./inventory.yml) file with the host/s information.

```bash
$ make ANSIBLE_INVENTORY=/a/path LIMIT=testing_hosts_linux NR_OTEL_COLLECTOR_MEMORY_LIMIT=100 NR_OTEL_COLLECTOR_OTLP_ENDPOINT=staging-otlp.nr-data.net:4317
```
