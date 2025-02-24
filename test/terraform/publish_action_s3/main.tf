###################################################################################
# State Backend
###################################################################################
terraform {
  backend "s3" {
    bucket         = "agent-control-terraform-states"
    dynamodb_table = "agent-control-terraform-states"
    key            = "publish_action_s3/terraform-states-backend.tfstate"
    region         = "us-east-2"
  }
}

#################################################################################
# S3 Bucket for public action
#################################################################################
resource "aws_s3_bucket" "my_bucket" {
  bucket = var.my_bucket_name
}

resource "aws_s3_bucket_policy" "my_bucket_policy" {
  bucket = aws_s3_bucket.my_bucket.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Principal = "*"
        Action = [
          "s3:PutObject",
          "s3:ListBucket",
          "s3:GetObject"
        ],
        Resource = [
          "arn:aws:s3:::${aws_s3_bucket.my_bucket.id}",
          "arn:aws:s3:::${aws_s3_bucket.my_bucket.id}/*"
        ]
        Condition = {
          StringEquals = {
            "aws:sourceVpce" = aws_vpc_endpoint.s3.id  // VPC Endpoint ID
          }
        }
      }
    ]
  })
}

#################################################################################
# VPC endpoint to access from EC2
#################################################################################

data "aws_route_tables" "selected" {
  vpc_id = var.vpc_id
}

resource "aws_vpc_endpoint" "s3" {
  vpc_id            = var.vpc_id
  service_name      = "com.amazonaws.${var.region}.s3"
  vpc_endpoint_type = "Gateway"
  route_table_ids   = data.aws_route_tables.selected.ids
}

#################################################################################
# README for the bucket
#################################################################################

resource "aws_s3_object" "example" {
  bucket = aws_s3_bucket.my_bucket.id
  key    = "README.md" # The name of the file in the bucket
  content = "This bucket is meant to be used to test the download.newrelic.com repository (currently publish action)"
}
