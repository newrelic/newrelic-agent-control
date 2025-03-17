terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.48"
    }

    newrelic = {
      source = "newrelic/newrelic"
    }
  }

  backend "s3" {
    bucket         = "agent-control-terraform-states"
    dynamodb_table = "agent-control-terraform-states"
    key            = "k8s_production/terraform-states-backend.tfstate"
    region         = "us-east-2"
  }
}

provider "aws" {
  region = "us-east-2"
  default_tags {
    tags = {
      "owning_team" =  "AGENT-CONTROL"
      "purpose"     = "development-agent-control-environment"
    }
  }
}

provider "newrelic" {
  account_id = var.account_id
  api_key    = var.api_key
  region     = "Staging" # To change to US when production is stable
}