# This is a Testing cluster without any alerts neither CI workflows.
module "eks_cluster" {
  source               = "../modules/eks_cluster"
  canary_name          = "Agent_Control_Testing"
  cluster_desired_size = 0
  cluster_max_size     = 1
  cluster_min_size     = 0
}

provider "kubernetes" {
  host                   = module.eks_cluster.cluster_endpoint
  cluster_ca_certificate = base64decode(module.eks_cluster.cluster_ca_cert)
  token                  = module.eks_cluster.cluster_auth_token
}