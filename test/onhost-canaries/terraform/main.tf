terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.48"
    }
  }
}

provider "aws" {
  region = "us-east-2"
}

variable "system_identity_client_id" {
  description = "NR System Identity Client ID"
  type = string
}

variable "license_key" {
  description = "NR License Key"
  type = string
}

variable "ec2_prefix" {
  description = "Prefix for EC2 instances"
  type = string
}

variable "inventory_output" {
  description = "Path to write the inventory file"
  type = string
}

variable "repository_endpoint" {
  description = "Agent Control Repository Endpoint"
  type = string
}

locals {
  ec2_otels = {
    "amd64:ubuntu22.04" = {
      ami             = "ami-0884d2865dbe9de4b"
      subnet          = "subnet-00aa02e6d991b478e"
      security_groups = ["sg-04ae18f8c34a11d38"]
      key_name        = "caos-dev-arm"
      instance_type   = "t3a.small"
      username        = "ubuntu"
      platform        = "linux"
      python          = "/usr/bin/python3"
      // We don't install the otel collector on the onhost canaries,
      // but the tag is required by the terraform module
      tags = {
        "otel_role" = "agent"
      }
    }
  }
}

module "agent_control-canary-env-provisioner" {
  source             = "git::https://github.com/newrelic-experimental/env-provisioner//terraform/otel-ec2"
  ec2_prefix         = var.ec2_prefix
  ec2_filters        = ""
  nr_license_key     = var.license_key
  otlp_endpoint      = "staging-otlp.nr-data.net:4317"
  pvt_key            = "~/.ssh/caos-dev-arm.cer"
  ssh_pub_key        = "AAAAB3NzaC1yc2EAAAADAQABAAABAQDH9C7BS2XrtXGXFFyL0pNku/Hfy84RliqvYKpuslJFeUivf5QY6Ipi8yXfXn6TsRDbdxfGPi6oOR60Fa+4cJmCo6N5g57hBS6f2IdzQBNrZr7i1I/a3cFeK6XOc1G1tQaurx7Pu+qvACfJjLXKG66tHlaVhAHd/1l2FocgFNUDFFuKS3mnzt9hKys7sB4aO3O0OdohN/0NJC4ldV8/OmeXqqfkiPWcgPx3C8bYyXCX7QJNBHKrzbX1jW51Px7SIDWFDV6kxGwpQGGBMJg/k79gjjM+jhn4fg1/VP/Fx37mAnfLqpcTfiOkzSE80ORGefQ1XfGK/Dpa3ITrzRYW8xlR caos-dev-arm"
  inventory_template = "../ansible/inventory-template.tmpl"
  inventory_output   = var.inventory_output
  ansible_playbook   = "-e system_identity_client_id=${var.system_identity_client_id} -e nr_license_key=${var.license_key} -e repo_endpoint=${var.repository_endpoint} ../ansible/install_ac_with_basic_config.yml"
  ec2_otels          = local.ec2_otels
}
