# Local execution using vagrant

## Requirements
- Vagrant (+Virtualbox).

## Set up local environment

Spin up a vagrant VM. You may use a `Vagrantfile` similar to:

```ruby
Vagrant.configure("2") do |config|
  config.vm.box = "bento/ubuntu-22.04"
  config.vm.provider "virtualbox"
  require 'time'
  current_time = Time.now.strftime("%Y%m%d%H%M%S")
  config.vm.hostname = "vg-#{current_time}"
  config.vm.provision "shell", privileged: false, inline: <<-SHELL
    sudo apt-get update
    sudo apt-get install -y bash-completion build-essential

    # Ansible
    curl https://bootstrap.pypa.io/get-pip.py -o get-pip.py && \
    python3 get-pip.py  && \
    python3 -m pip install ansible==10.7.0 jmespath==1.0.1 pywinrm==0.5.0
  SHELL
end
```

The Vagrant VM can be launched using `vagrant up` from the same folder where the `Vagrantfile` was placed. Check out [Vagrant's getting started](https://developer.hashicorp.com/vagrant/tutorials/getting-started) for details.


## Execution
```sh
# In case you want to execute the e2e using the current commit AC, run the package creation with a tag
# that doesn't match with any in the prod repo. 
GORELEASER_CURRENT_TAG=9.0.0 NR_RELEASE_TAG=9.0.0 goreleaser release --skip sign --skip publish --skip validate --clean

make test/onhost-e2e \
  PACKAGE_VERSION="<9.0.0(build from current branch) or upstream prod version>"\
  NR_LICENSE_KEY=$LICENSE_KEY \
  NEW_RELIC_ACCOUNT_ID=$ACCOUNT_ID \
  NEW_RELIC_API_KEY=$API_REST_KEY \
  NR_ORGANIZATION_ID=$ORGANIZATION_ID \
  NR_SYSTEM_IDENTITY_CLIENT_ID=$SYSTEM_IDENTITY_CLIENT_ID \
  NR_SYSTEM_IDENTITY_PRIVATE_KEY=$SYSTEM_IDENTITY_PRIVATE_KEY \
  ANSIBLE_PLAYBOOK=<test playbook>.yaml
```
