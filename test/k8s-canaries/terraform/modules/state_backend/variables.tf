variable "bucket_name" {
  description = "The name of the S3 bucket to store Terraform state"
  type        = string
  default     = "agent-control-terraform-states"
}

variable "dynamodb_table_name" {
  description = "The name of the DynamoDB table used for state locking"
  type        = string
  default     = "agent-control-terraform-states"
}
