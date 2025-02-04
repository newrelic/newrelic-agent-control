output "cluster" {
  value = {
     name     = aws_eks_cluster.ekscluster.name
     endpoint = aws_eks_cluster.ekscluster.endpoint
  }
}
