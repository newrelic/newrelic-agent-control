output "ekscluster" {
  value = {
    aws_eks_cluster = {
      ekscluster = {
        name     = aws_eks_cluster.ekscluster.name
        endpoint = aws_eks_cluster.ekscluster.endpoint
      }
    }
  }
}
