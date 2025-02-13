#######################################
# S3 Bucket for public action
#######################################
variable "region" {
  default = "us-east-2"
}

variable "my_bucket_name" {
  description = "The name of the S3 bucket"
  type        = string
  default     = "agent-control-package-repository-testing"
}

variable "vpc_id" {
  description = "The ID of the VPC allowed to access the S3 bucket"
  type        = string
  default     = "vpc-0f62d8a55c8d9ad61"
}
