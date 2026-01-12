
data "aws_eks_cluster" "ekscluster" {
  name = aws_eks_cluster.ekscluster.name

  depends_on = [
    aws_eks_cluster.ekscluster
  ]
}

data "aws_eks_cluster_auth" "ekscluster" {
  name = aws_eks_cluster.ekscluster.name

  depends_on = [
    aws_eks_cluster.ekscluster
  ]
}

# Retrieves the aws_auth configmap created by the EKS Fargate module that has the RBAC for the role used by terraform
data "kubernetes_config_map" "aws_auth" {
  metadata {
    name      = "aws-auth"
    namespace = "kube-system"
  }

  depends_on = [
    aws_eks_cluster.ekscluster
  ]
}

locals {
  new_role_mapping = <<EOF
- rolearn: ${var.fargate_iam_role_arn}
  username: ${var.fargate_iam_role_user}
  groups:
    - system:masters
EOF

  # Check if the role mapping already exists in the current mapRoles
  role_exists = can(regex("${var.fargate_iam_role_arn}", data.kubernetes_config_map.aws_auth.data["mapRoles"]))

  # Construct the updated mapRoles by only adding the new role if it's not already present
  updated_map_roles = local.role_exists ? data.kubernetes_config_map.aws_auth.data["mapRoles"] : "${data.kubernetes_config_map.aws_auth.data["mapRoles"]}\n${local.new_role_mapping}"
}

# The aws-auth RBAC configmap is updated with the new user keeping also the old one.
resource "kubernetes_config_map_v1_data" "aws_auth" {
  force = true

  metadata {
    name      = "aws-auth"
    namespace = "kube-system"
  }

  data = {
    mapRoles = local.updated_map_roles
  }

  lifecycle {
    ignore_changes = [
      data["mapUsers"],
      data["mapAccounts"]
    ]
    create_before_destroy = true
  }

  depends_on = [
    aws_eks_cluster.ekscluster
  ]
}

resource "kubernetes_cluster_role" "fargate_role" {
  metadata {
    name = "fargate-role"
  }

  rule {
    api_groups = [""]
    resources  = ["pods", "services", "deployments"]
    verbs      = ["get", "list", "watch", "create", "update", "delete"]
  }

  depends_on = [
    aws_eks_cluster.ekscluster
  ]
}

resource "kubernetes_cluster_role_binding" "fargate_role" {
  metadata {
    name = "fargate-role-binding"
  }

  role_ref {
    api_group = "rbac.authorization.k8s.io"
    kind      = "ClusterRole"
    name      = kubernetes_cluster_role.fargate_role.metadata[0].name
  }

  subject {
    kind      = "User"
    name      = var.fargate_iam_role_user
    api_group = "rbac.authorization.k8s.io"
  }

  depends_on = [
    aws_eks_cluster.ekscluster
  ]
}
