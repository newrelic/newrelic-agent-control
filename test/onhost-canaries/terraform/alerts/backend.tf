terraform {
  required_providers {
    newrelic = {
      source = "newrelic/newrelic"
    }
  }

  backend "s3" {
    bucket         = "agent-control-terraform-states"
    dynamodb_table = "agent-control-terraform-states"
    region         = "us-east-2"
  }
}

provider "newrelic" {
  account_id = var.account_id
  api_key    = var.api_key
  region     = var.nr_region
}
