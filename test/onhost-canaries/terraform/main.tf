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
  ec2_configs = {
    "amd64:ubuntu22.04" = {
      metrics_attributes_id = "${var.ec2_prefix}-amd64-ubuntu22-04"
      ami             = "ami-0884d2865dbe9de4b"
      subnet          = "subnet-00aa02e6d991b478e"
      security_groups = ["sg-04ae18f8c34a11d38"]
      key_name        = "caos-dev-arm"
      instance_type   = "t3a.small"
      username        = "ubuntu"
    }
  }

  hostnames = [for k, v in local.ec2_configs : v.metrics_attributes_id ]
}

module "ec2_provisioner" {
  source  = "./modules/ec2-provisioner"

  license_key         = var.license_key
  inventory_output    = var.inventory_output
  ec2_prefix          = var.ec2_prefix
  ec2_configs         = local.ec2_configs
  ssh_key_file        = "~/.ssh/caos-dev-arm.cer"
  repository_endpoint = var.repository_endpoint
  ansible_playbook    = "../ansible/install_ac_with_basic_config.yml"
}

variable "account_id" {
  description = "New Relic Account ID"
  type = string
}
variable "api_key" {
  description = "New Relic API Key"
  type = string
}

variable "slack_webhook_url" {
  description = "Slack Webhook URL where alerts notifications will be sent"
  type = string
}

variable "nr_region" {
  description = "New Relic Region"
  type = string
}

module "alerts" {
  source = "../../terraform/modules/nr_alerts"

  for_each = toset(local.hostnames)

  api_key    = var.api_key
  account_id = var.account_id
  slack_webhook_url = var.slack_webhook_url

  policies_prefix = "Agent Control canaries metric monitoring"

  region = var.nr_region
  instance_id = each.value
  conditions = [
    {
      name = "CPU usage (percentage)"
      metric = "cpuPercent"
      sample = "ProcessSample"
      threshold = 0.06
      duration = 3600
      operator = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name = "CPU usage (percentage)"
      metric = "cpuPercent"
      sample = "ProcessSample"
      threshold = 0
      duration = 3600
      operator = "below_or_equals"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name = "Memory usage (bytes)"
      metric = "memoryResidentSizeBytes"
      sample = "ProcessSample"
      threshold = 14000000
      duration = 600
      operator = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name = "Memory usage (bytes)"
      metric = "memoryResidentSizeBytes"
      sample = "ProcessSample"
      threshold = 0
      duration = 600
      operator = "below_or_equals"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name = "Disk usage (read bytes)"
      metric = "ioTotalReadBytes"
      sample = "ProcessSample"
      threshold = 500000
      duration = 600
      operator = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name = "Disk usage (written bytes)"
      metric = "ioTotalWriteBytes"
      sample = "ProcessSample"
      threshold = 10000
      duration = 600
      operator = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
  ]
}