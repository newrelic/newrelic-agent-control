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
    region = "us-east-2"
  }
}

provider "aws" {
  region = "us-east-2"
}

# Configure the New Relic provider.
provider "newrelic" {
  account_id = var.account_id
  api_key    = var.api_key
  region     = var.nr_region
}