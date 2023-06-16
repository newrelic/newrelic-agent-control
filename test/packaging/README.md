# Packaging tests

Run packaging tests:
```shell
# Default values
NR_LICENSE_KEY=nr_license_key make test/packaging

# Limit to linux
NR_LICENSE_KEY=nr_license_key LIMIT=testing_hosts_linux make test/packaging
```

## Required parameters

* `NR_LICENSE_KEY`: New Relic license key.

## Optional parameters

* `ANSIBLE_INVENTORY`: Path of the Ansible inventory file (default: inventory.ec2).
* `LIMIT`: Ansible inventory group name to limit the execution for (default: testing_hosts_linux).
* `ANSIBLE_FORKS`: Maximum number of concurrent Ansible forks (default: 5).
