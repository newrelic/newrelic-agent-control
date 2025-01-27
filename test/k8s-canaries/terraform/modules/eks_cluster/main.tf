# EKS Cluster
resource "aws_eks_cluster" "ekscluster" {
  name     = "${var.canary_name}-Cluster"
  role_arn = aws_iam_role.cluster.arn
  version  = var.k8s_version

  vpc_config {
    subnet_ids              = flatten([var.base_subnet_ids[*]])

    endpoint_private_access   = true
    endpoint_public_access    = false
  }

  depends_on = [
    aws_iam_role_policy_attachment.cluster_AmazonEKSClusterPolicy
  ]
}


# EKS Addons
resource "aws_eks_addon" "volume-provisioner" {
  cluster_name                = aws_eks_cluster.ekscluster.name
  addon_name                  = "aws-ebs-csi-driver"
  addon_version               = var.aws_eks_addon_version
  resolve_conflicts_on_create = "OVERWRITE"
}


# EKS Cluster IAM Role
resource "aws_iam_role" "cluster" {
  name = "${var.canary_name}-EKS_Cluster_Role"

  assume_role_policy = jsonencode({
    "Version" : "2012-10-17",
    "Statement" : [
      {
        "Effect" : "Allow",
        "Principal" : {
          "Service" : "eks.amazonaws.com"
        },
        "Action" : "sts:AssumeRole"
      }
    ]
  })
}

resource "aws_iam_role_policy_attachment" "cluster_AmazonEKSClusterPolicy" {
  policy_arn = "arn:aws:iam::aws:policy/AmazonEKSClusterPolicy"
  role       = aws_iam_role.cluster.name
}


# EKS Cluster Security Group
resource "aws_security_group" "eks_cluster" {
  name        = "${var.canary_name} EKS Cluster Security Group"
  description = "Cluster communication with worker nodes"
  vpc_id      = "${var.base_vpc_id}"

  tags = {
    Name = "${var.canary_name} EKS Cluster Security Group"
  }
}

resource "aws_security_group_rule" "cluster_inbound" {
  description              = "Allow worker nodes to communicate with the cluster API Server"
  from_port                = 443
  protocol                 = "tcp"
  security_group_id        = aws_security_group.eks_cluster.id
  cidr_blocks              = ["10.10.32.0/20"]
  to_port                  = 443
  type                     = "ingress"
}

resource "aws_security_group_rule" "cluster_outbound" {
  description              = "Allow cluster API Server to communicate with the worker nodes"
  from_port                = 1024
  protocol                 = "tcp"
  security_group_id        = aws_security_group.eks_cluster.id
  source_security_group_id = aws_security_group.eks_nodes.id
  to_port                  = 65535
  type                     = "egress"
}


resource "aws_eks_node_group" "eks-nodegroup" {
  cluster_name    = aws_eks_cluster.ekscluster.name
  node_group_name = "${var.canary_name}-Node_Group"
  node_role_arn   = aws_iam_role.node.arn
  subnet_ids      = flatten([var.base_subnet_ids[*]])
  # Pining version to follow the cluster during the upgrades.
  version = var.k8s_version

  scaling_config {
    desired_size = var.cluster_desired_size
    max_size     = var.cluster_max_size
    min_size     = var.cluster_min_size
  }

  ami_type       = var.nodes_ami_type
  capacity_type  = "ON_DEMAND" # ON_DEMAND, SPOT
  disk_size      = 20
  instance_types = ["${var.nodes_instance_type}"]

  depends_on = [
    aws_iam_role_policy_attachment.node_AmazonEKSWorkerNodePolicy,
    aws_iam_role_policy_attachment.node_AmazonEKS_CNI_Policy,
    aws_iam_role_policy_attachment.node_AmazonEC2ContainerRegistryReadOnly,
    aws_iam_role_policy_attachment.node_AmazonEBSCSIDriverPolicy,
    aws_iam_role_policy_attachment.node_AmazonECRReadOnly,
  ]
}


# EKS Node IAM Role
resource "aws_iam_role" "node" {
  name = "${var.canary_name}-EKS_Worker_Role"

  assume_role_policy = jsonencode({
    "Version" : "2012-10-17",
    "Statement" : [
      {
        "Effect" : "Allow",
        "Principal" : {
          "Service" : "ec2.amazonaws.com"
        },
        "Action" : "sts:AssumeRole"
      }
    ]
  })
}

resource "aws_iam_policy" "node" {
  name        = "${var.canary_name}-EKS_Worker_Policy"
  description = "This policy is attach to EKS Nodes"

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        # Allows the node to access all ECR registries
        Effect = "Allow"
        "Resource" : "*"
        "Action" : [
          "ecr:BatchCheckLayerAvailability",
          "ecr:BatchGetImage",
          "ecr:GetDownloadUrlForLayer",
          "ecr:GetAuthorizationToken"
        ],
      }
    ]
  })
}

resource "aws_iam_role_policy_attachment" "node_AmazonEKSWorkerNodePolicy" {
  policy_arn = "arn:aws:iam::aws:policy/AmazonEKSWorkerNodePolicy"
  role       = aws_iam_role.node.name
}

resource "aws_iam_role_policy_attachment" "node_AmazonEKS_CNI_Policy" {
  policy_arn = "arn:aws:iam::aws:policy/AmazonEKS_CNI_Policy"
  role       = aws_iam_role.node.name
}

resource "aws_iam_role_policy_attachment" "node_AmazonEC2ContainerRegistryReadOnly" {
  policy_arn = "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryReadOnly"
  role       = aws_iam_role.node.name
}

resource "aws_iam_role_policy_attachment" "node_AmazonEBSCSIDriverPolicy" {
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonEBSCSIDriverPolicy"
  role       = aws_iam_role.node.name
}

resource "aws_iam_role_policy_attachment" "node_AmazonECRReadOnly" {
  policy_arn = aws_iam_policy.node.arn
  role       = aws_iam_role.node.name
}


# EKS Node Security Group
resource "aws_security_group" "eks_nodes" {
  name        = "${var.canary_name} EKS Node Security Group"
  description = "Security group for all nodes in the cluster"
  vpc_id      = "${var.base_vpc_id}"

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = {
    Name                                               = "${var.canary_name} EKS Node Security Group"
    "kubernetes.io/cluster/${var.canary_name}-cluster" = "owned"
  }
}

resource "aws_security_group_rule" "nodes_internal" {
  description              = "Allow nodes to communicate with each other"
  from_port                = 0
  protocol                 = "-1"
  security_group_id        = aws_security_group.eks_nodes.id
  source_security_group_id = aws_security_group.eks_nodes.id
  to_port                  = 65535
  type                     = "ingress"
}

resource "aws_security_group_rule" "vpn_k8s_api" {
  description              = "Allow all hosts to connect to the k8s api (it requires the host to be in our vpn)"
  from_port                = 443
  protocol                 = "tcp"
  security_group_id        = aws_eks_cluster.ekscluster.vpc_config[0].cluster_security_group_id
  cidr_blocks              = ["0.0.0.0/0"]
  to_port                  = 443
  type                     = "ingress"
}

resource "aws_security_group_rule" "nodes_cluster_inbound" {
  description              = "Allow worker Kubelets and pods to receive communication from the cluster control plane"
  from_port                = 1025
  protocol                 = "tcp"
  security_group_id        = aws_security_group.eks_nodes.id
  source_security_group_id = aws_security_group.eks_cluster.id
  to_port                  = 65535
  type                     = "ingress"
}
