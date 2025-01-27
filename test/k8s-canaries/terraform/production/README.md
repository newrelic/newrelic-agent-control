# Production Cluster creation

This root module uses the [eks_cluster](../modules/eks_cluster/README.md) child module to create the Production EKS Cluster.

**IMPORTANT**: All state and locks for this terraform module are held in the S3 bucket and DynamoDB table created by the [infra_setup](../states_setup/README.md).

### Usage:
  ```
  $ terraform init
  $ terraform apply
  ```

### Setting up kubeconfig to connect to EKS Cluster
1. Retrieve the name of the EKS Cluster:
  ```
  $ aws eks list-clusters
  ```
2. Setup kubeconfig to connect to the new cluster:
  ```
  $ aws eks update-kubeconfig --name <cluster_name>
  ```  
3. Check if the new configuration is active:
  ```
  $ kubectl cluster-info
  ```  
