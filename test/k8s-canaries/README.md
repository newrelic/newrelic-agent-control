# Newrelic Agent Control k8s canaries
The purpose of this tool is to create the EKS Clusters to run our canaries.

## Local Usage

- Log-in to the aws cli and export the AWS_PROFILE env variable for terraform to have access.
- A user with IAM policy creation and security rights needs to be used to run cluster creation.
- Follow the instructions in [states_setup](../terraform/states_setup/README.md) in order to create the S3 bucket and DynamoDB used to save the terraform states for the canaries.
- Once the state bucket is created, [staging](terraform/staging/README.md) defines the steps to create the staging K8s Cluster.
- Any new Cluster we want to create will need a new root module similar to the Staging one.

## Usage on pipelines

### Terraform init and apply

The idea is to call this target any time there are changes in the tf files and will apply the changes if correct.
There are 2 clusters created and when calling the target the CANARY_DIR needs to be provided, the canary dirs are:
- staging
- production

The default terraform dir for where the terraform modules are taken from is:
`TERRAFORM_DIR := ./terraform`

```bash
$ make CANARY_DIR=staging test/k8s-canaries/terraform-apply
```

If this target is called from the root of this repository, the TERRAFORM_DIR should be overwritten to point to the relative path:
```bash
$ make TERRAFORM_DIR=test/k8s-canaries/terraform CANARY_DIR=staging test/k8s-canaries/terraform-apply
```

### Helm Upgrade for nightlies and prereleases

This target will add the helm repo if not present and upgrade (or install) the helm repo with the agent-control.yml values present on this folder.
The agent-control pod will always pull the image on every upgrade because there is a random deployment-key annotation added each time.

The default helm dir where there is the default values file to apply is:
`HELM_DIR := ./helm`

```bash
$ make NR_LICENSE_KEY=xxx CLUSTER_NAME=my-cluster IMAGE_TAG=nightly test/k8s-canaries/helm-upgrade
```

If this target is called from the root of this repository, the HELM_DIR should be overwritten to point to the relative path:
```bash
$ make HELM_DIR=test/k8s-canaries/helm NR_LICENSE_KEY=xxx CLUSTER_NAME=my-cluster IMAGE_TAG=nightly test/k8s-canaries/helm-upgrade
```
