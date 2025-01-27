terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 3.0"
    }
  }

  # Used to save the local tfstate after the bucket is created.
  # On first execution it should be commented to allow creation of the bucket,
  # because this will fail to save the state on a bucket that was still not created.
  backend s3 {
    bucket         = "agent-control-canary-states"
    dynamodb_table = "agent-control-canary-states"
    key            = "foundations/state_framework.tfstate"
    region         = "us-east-2"
  }
}

provider "aws" {
  region = "us-east-2"
  default_tags {
    tags = {
      "owning_team" = "AGENT-CONTROL"
      "purpose"     = "development-agent-control-environment"
    }
  }
}

data "aws_region" "current" {}

module "state_backend" {
  source              = "../modules/state_backend"
  bucket_name         = "agent-control-canary-states"
  dynamodb_table_name = "agent-control-canary-states"
}
