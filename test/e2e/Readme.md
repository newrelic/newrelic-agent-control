# Local execution using vagrant

## Requirements
- Vagrant (+Parallels and [parallels plugin](https://kb.parallels.com/122843) for Mac M1/M2).
- Ansible `brew install ansible`
- The newrelic install role requires the following collections: `ansible-galaxy collection install ansible.windows ansible.utils`.

## Set up local environment

Spin up a vagrant VM. You may use a `Vagrantfile` similar to:

```ruby
Vagrant.configure("2") do |config|
  config.vm.box = "bento/ubuntu-22.04-arm64"
  config.vm.define "ubuntu22"
  config.vm.hostname = "ubuntu22"
  config.vm.network "private_network", ip: "10.0.2.27"
  config.vm.provider :parallels do |v|
      v.name = "ubuntu22"
  end
end
```

Get the ssh-config to connect to the VM and store it a location of your choice (`/path/to/vagrant-ssh-config`):

```sh
$ vagrant ssh-config > /path/to/vagrant-ssh-config
```

You'll get a config file similar to:

```
Host ubuntu22
  HostName 10.211.55.16
  User vagrant
  Port 22
  UserKnownHostsFile /dev/null
  StrictHostKeyChecking no
  PasswordAuthentication no
  IdentityFile /Users/YOUR-USER/vagrant/ubuntu-22/.vagrant/machines/ubuntu22/parallels/private_key
  IdentitiesOnly yes
  LogLevel FATAL
  PubkeyAcceptedKeyTypes +ssh-rsa
  HostKeyAlgorithms +ssh-rsa
```

Create an custom inventory file `inventory.yml`. Example:

```yaml
testing_hosts_linux:
  hosts:
    ubuntu22:
      ansible_connection: ssh
      ansible_ssh_common_args: "-F /path/to/vagrant-ssh-config"
```

You can check connectivity to the VM using:

```sh
$ ansible all -i /path/to/your/inventory.yml -m ping
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
  ANSIBLE_INVENTORY=/path/to/your/inventory.yml \
  ANSIBLE_PLAYBOOK=test.yaml
```

You can customize `ANSIBLE_PLAYBOOK` to to execute one of the testing playbooks only. E.g.: `migration_script_execution.yaml`.
