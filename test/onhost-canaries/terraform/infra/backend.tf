terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.48,  < 6.0"
    }
  }

  backend "s3" {
    bucket         = "agent-control-terraform-states"
    dynamodb_table = "agent-control-terraform-states"
    region         = "us-east-2"
  }
}

provider "aws" {
  region = "us-east-2"
}
