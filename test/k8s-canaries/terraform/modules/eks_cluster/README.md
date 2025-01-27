# Cluster creation

This module builds basic infrastructure to deploy canary resources. When applied, terraform will create:
* 2 Security Groups (EKS and EKS Nodes)
* 1 EKS Cluster (On the Private subnet)
* 2 IAM Roles (EKS and EKS nodes)

This module should be used as a child module by any new cluster we create. Each root module including this child module will be in charge of using the states' backend.
