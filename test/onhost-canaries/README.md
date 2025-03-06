# Newrelic Agent Control onhost canaries

The purpose of this tool is to create the EC2 instances to run our onhost canaries.

## Folder structure

We have the following structure:

```bash
├── ansible
└── terraform
    ├── modules
    ├── environments
    ├── backend.tf
    ├── main.tf
    └── states_setup
```

The `ansible` folder contains the instructions to install agent control on the deployed ec2 instances.

The `terraform` folder contains the instructions to create a single or a group of ec2 instances.
Notice that we don't have root modules. We have a `main.tf` and `backend.tf` in the root folder.
That is because we manage different environments with configuration files stored under `environments/{{ env name }}`.
For instance, `environments/production` contains the different variables required to launch the production canaries.

That has a little inconvenient, `terraform init` will fail because the state changed. Hence, we need to use `terraform init -reconfigure`.

## Local Usage

1. Get AWS access for terminal

    Log-in to the aws cli and export the AWS_PROFILE env variable for terraform to have access.

    ```bash
    aws sso login --profile "profile name"
    export AWS_PROFILE="profile name"
    ```

2. Prepare S3 and DynamodDB for terraform states

    Follow the instructions in [states_setup](../terraform/states_setup/README.md) in order to create the S3 bucket and DynamoDB used to save the terraform states for the canaries.

3. Configure the SSH key

    You need to have `~/.ssh/caos-dev-arm.cer`. You can get it from AWS Secrets Manager.

4. Create canaries

    If you are executing it from the root folder of the project, you will have to modify the `TERRAFORM_DIR` and `ANSIBLE_FOLDER` environment variables to point to the correct location. Otherwise, you don't need to explicitly set them.

    To select to which "environment" should the canaries send the information to, use the `ENVIRONMENT` environment variable with "staging" or "production".

    ```bash
    TERRAFORM_DIR=test/onhost-canaries/terraform ONHOST_ANSIBLE_FOLDER=test/onhost-canaries/ansible ENVIRONMENT=staging NR_LICENSE_KEY=xxx NR_SYSTEM_IDENTITY_CLIENT_ID=xxx NR_SYSTEM_IDENTITY_PRIVATE_KEY=xxx make test/onhost-canaries/terraform-apply
    ```

That's it. If you want to create another set of canaries, just create a new folder under `environments` and populate the config files.
Apart from `terraform-plan` and `terraform-apply` make targets, we also have `terraform-destroy` in case we need to remove the canaries. 

## Usage on pipelines

In that case, we will use fargate runner to execute make targets. The commands will be very similar, but remember that secrets come from AWS. It will also add the ssh key into the instance.

Example for terraform plan:

```yaml
- name: Plan onhost staging canary changes
  uses: newrelic/fargate-runner-action@main
  with:
    container_make_target: "TERRAFORM_DIR=test/onhost-canaries/terraform ENVIRONMENT=staging test/onhost-canaries/terraform-plan"
    etc: ...
```

Example for terraform apply:

```yaml
- name: Plan onhost staging canary changes
  uses: newrelic/fargate-runner-action@main
  with:
    container_make_target: "TERRAFORM_DIR=test/onhost-canaries/terraform ONHOST_ANSIBLE_FOLDER=test/onhost-canaries/ansible ENVIRONMENT=staging test/onhost-canaries/terraform-apply"
    etc: ...
```