variable "ec2_prefix" {
  default = ""
}
variable "ec2_filters" {
  default = ""
}
variable "nr_license_key" {
  default = ""
}
variable "otlp_endpoint" {
  default = ""
}
variable "pvt_key" {
  default = ""
}
variable "ssh_pub_key" {
  default = ""
}
variable "ansible_playbook" {
  default = ""
}

variable "region" {
  default = "us-east-2"
}

provider "aws" {
  region = var.region
}

variable "inventory_output" {
  default = "./inventory.ec2"
}

module "super_agent-env-provisioner" {
  source             = "git::https://github.com/newrelic-experimental/env-provisioner//terraform/otel-ec2?ref=NR-217272_add_ec2_filters"
  ec2_prefix         = var.ec2_prefix
  ec2_filters        = var.ec2_filters
  nr_license_key     = var.nr_license_key
  otlp_endpoint      = var.otlp_endpoint
  pvt_key            = var.pvt_key
  ssh_pub_key        = var.ssh_pub_key
  inventory_template = "${path.module}/inventory.tmpl"
  inventory_output   = var.inventory_output
  ansible_playbook   = var.ansible_playbook
}

output "check_vars" {
  value = var.inventory_output
}
