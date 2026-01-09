output "cluster" {
  value = {
     name     = aws_eks_cluster.ekscluster.name
     endpoint = aws_eks_cluster.ekscluster.endpoint
  }
}

output "cluster_endpoint" {
  value = aws_eks_cluster.ekscluster.endpoint
}

output "cluster_ca_cert" {
  value = aws_eks_cluster.ekscluster.certificate_authority[0].data
}

output "cluster_auth_token" {
  value     = data.aws_eks_cluster_auth.ekscluster.token
  sensitive = true
}
