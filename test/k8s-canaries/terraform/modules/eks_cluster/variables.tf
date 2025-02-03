variable "canary_name" {
  type    = string
  default = "Agent_Control_Canaries"
}
# By Default we use the VPC created by Security
variable "base_vpc_id" {
  type    = string
  default = "vpc-0f62d8a55c8d9ad61"
}
# By Default we use the subnets created by Security
variable "base_subnet_ids" {
  default = ["subnet-00aa02e6d991b478e", "subnet-02848327c6307b4dc", "subnet-0792c40be9a9e023c"]
}

# EKS Cluster definitions (When upgrading only one minor version upgrading is supported by AWS at a time,
# For example, you can upgrade from 1.29 to 1.30, but not directly to 1.31)
variable "k8s_version" {
  description = "Kubernetes version"
  type        = string
  default     = "1.31"
}
#  Amazon EKS aws-ebs-csi-driver add-on (https://docs.aws.amazon.com/es_es/eks/latest/userguide/ebs-csi.html)
#  Execute ·aws eks describe-addon-versions --addon-name aws-ebs-csi-driver" to see if needs update when updating the cluster version
variable "aws_eks_addon_version" {
  description = "aws_eks_addon version"
  type        = string
  default     = "v1.29.1-eksbuild.1"
}
variable "nodes_instance_type" {
  type    = string
  default = "t3.xlarge" # 4 vCPUs, 16Gb of RAM: https://aws.amazon.com/ec2/instance-types/
}
variable "nodes_ami_type" {
  type    = string
  default = "AL2_x86_64" # AL2_x86_64, AL2_x86_64_GPU, AL2_ARM_64, CUSTOM
}
variable "node_volume_size" {
  type    = number
  default = 20
}
variable "cluster_desired_size" {
  type        = number
  default     = 2
}
variable "cluster_max_size" {
  type        = number
  default     = 3
}
variable "cluster_min_size" {
  type        = number
  default     = 2
}
