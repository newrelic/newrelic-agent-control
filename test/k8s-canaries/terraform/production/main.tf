  # Use the EKS cluster module
module "eks_cluster" {
  source               = "../modules/eks_cluster"
  canary_name          = "Agent_Control_Canaries_Production"
  cluster_desired_size = 2
  cluster_max_size     = 3
  cluster_min_size     = 2
}
