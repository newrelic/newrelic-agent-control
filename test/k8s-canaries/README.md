# Newrelic Agent Control k8s canaries
The purpose of this tool is to create the EKS Clusters to run our canaries.

## Local Usage

- Log-in to the aws cli and export the AWS_PROFILE env variable for terraform to have access.
- A user with IAM policy creation and security rights needs to be used to run cluster creation.
- Follow the instructions in [infra_setup](states_setup/README.md) in order to create the S3 bucket and DynamoDB used to save the terraform states for the canaries.
- Once the state bucket is created, [staging](k8s_staging/README.md) defines the steps to create the staging K8s Cluster.
- Any new Cluster we want to create will need a new root module similar to the Staging one.

## Usage on pipelines

There is a Make target to run the terraform init and apply in our pipelines. The idea is to call it any time there are changes in the tf files.

```bash
$ make CANARY_DIR=staging sync
```
