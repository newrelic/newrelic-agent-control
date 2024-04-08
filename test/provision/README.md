# Provision

The purpose of this tool is to easily deploy EC2 instances and run an Ansible
playbook to prepare them for further tests.

Deployment parameters like the EC2 AMIs and the Ansible playbook can be customized
in the Terraform file [./terraform/caos-linux.auto.tfvars.dist](./terraform/caos-linux.auto.tfvars.dist). Note
that already has some defaults of our environment.

Run provisioning:
```shell
// You must have valid AWS credentials at this point
EC2_FILTERS="[\"arm64:ubuntu22\"]" TAG_OR_UNIQUE_NAME=release_1.0 make provision
```

In the background, it automates the deployment of the [Otel-ec2](https://github.com/newrelic-experimental/otel-env-provisioner/tree/main/terraform/otel-ec2) Terraform module.

Destroy the provisioned instances:
```shell
// You must have valid AWS credentials at this point
make clean
```
