# This is a Testing cluster without any alerts neither CI workflows.
module "eks_cluster" {
  source               = "../modules/eks_cluster"
  canary_name          = "Agent_Control_Testing"
  cluster_desired_size = 0
  cluster_max_size     = 1
  cluster_min_size     = 0
}
