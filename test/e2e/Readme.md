# Local execution

Create an inventory file `inventory.yml`. Example using a vagrant machine:
```yaml
testing_hosts_linux:
  hosts:
    ubuntu:
      ansible_host: 10.211.55.51
      ansible_ssh_extra_args: "-o IdentitiesOnly=yes"
      ansible_ssh_user: vagrant
      # usually a file `private_key` under .vagrant/machines/...
      ansible_ssh_private_key_file: /path/to/vagrant/private/key
```

## Requirements
The newrelic.install role requires the following collections:
`ansible-galaxy collection install ansible.windows ansible.utils`

## Execution
```sh
make test/e2e \
  NR_LICENSE_KEY=$LICENSE_KEY \
  NEW_RELIC_ACCOUNT_ID=$ACCOUNT_ID \
  NEW_RELIC_API_KEY=$API_REST_KEY \
  NR_ORGANIZATION_ID=$ORGANIZATION_ID \
  REPOSITORY_ENDPOINT="https://nr-downloads-ohai-staging.s3.amazonaws.com/" \
  ANSIBLE_INVENTORY=./inventory.yml
```

