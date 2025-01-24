# States bucket and DynamoDB creation

This module deploys 2 resources:
* An S3 bucket to save state files from the different canaries
* A DynamoDB table to lock deployments

The resources created by this module are a dependency for all the other modules, they will use the S3 bucket created as the backend to save their terraform state. 
The following steps should be run only once before any other module:

### Usage:
  ```
  $ terraform init
  
  $ terraform plan
  
  $ terraform apply
  ```

The S3 provider backend should be commented on first execution to allow creating the bucket, or it will fail to save its own state since the bucket will still not be created.
