# Configure terraform.
terraform {
  required_version = "~> 1.0"
  required_providers {
    newrelic = {
      source = "newrelic/newrelic"
    }
  }
}

# Configure the New Relic provider.
provider "newrelic" {
  account_id = var.account_id
  api_key    = var.api_key
  region     = var.region
}
