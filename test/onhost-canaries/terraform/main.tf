variable "system_identity_client_id" {
  description = "NR System Identity Client ID"
  type        = string
}

variable "license_key" {
  description = "NR License Key"
  type        = string
}

variable "ec2_prefix" {
  description = "Prefix for EC2 instances"
  type        = string
}

variable "inventory_output" {
  description = "Path to write the inventory file"
  type        = string
}

variable "repository_endpoint" {
  description = "Agent Control Repository Endpoint"
  type        = string
}

variable "package_version" {
  description = "Agent Control Package Version"
  type        = string
}

variable "fleet_id" {
  description = "New Relic Fleet ID"
  type        = string
}

variable "otlp_endpoint" {
  description = "OTLP endpoint for the region"
  type        = string
}

variable "opamp_endpoint" {
  description = "OpAMP endpoint for the region"
  type        = string
}

variable "token_endpoint" {
  description = "Token renewal endpoint for the region"
  type        = string
}

variable "signature_validation_endpoint" {
  description = "Signature validation endpoint for the region"
  type        = string
}

variable "windows_password" {
  description = "windows ami password"
  type = string
}

locals {
  ec2_instances = {
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
    "amd64:windows_2022" = {
      // Custom Ami created from an image with winrm
      ami             = "ami-04382be054853bd1f"
      subnet          = "subnet-00aa02e6d991b478e"
      security_groups = ["sg-04ae18f8c34a11d38"]
      key_name        = "caos-dev-arm"
      instance_type   = "t3a.small"
      username        = "Administrator"
      platform        = "windows"
      python          = ""
      // We don't install the otel collector on the onhost canaries,
      // but the tag is required by the terraform module
      tags = {
        "otel_role" = "agent"
      }
    }
  }

  // To setup the alerts, we need to know the hostnames of the instances.
  // One option would be to wait for the ansible inventory to be created, but then
  // terraform won't be able to show all the resources that the apply operation
  // will create.
  // We decided to recompute the hostnames here as the "env-provisioner" module does.
  // If env-provisioner changes the way it computes the hostnames, we need to change
  // it here too. However, terraform plan will properly list all the resources that
  // will be created and we can spot any problems with the hostnames.
  // At the moment windows is not installing infra-agent so we only add the alerts to linux hosts
  // so we use the linux_hostnames variable instead of this one.
  hostnames = [for k, v in local.ec2_instances : "${var.ec2_prefix}-${replace(k, "/[:.]/", "-")}"]
  infra_staging = var.nr_region == "Staging" ? "true" : "false"
}

module "agent_control-canary-env-provisioner" {
  source             = "git::https://github.com/newrelic-experimental/env-provisioner//terraform/otel-ec2"
  ec2_prefix         = var.ec2_prefix
  ec2_filters        = ""
  nr_license_key     = var.license_key
  otlp_endpoint      = "staging-otlp.nr-data.net:4317"
  pvt_key            = "~/.ssh/caos-dev-arm.cer"
  windows_password   = var.windows_password
  ssh_pub_key        = "AAAAB3NzaC1yc2EAAAADAQABAAABAQDH9C7BS2XrtXGXFFyL0pNku/Hfy84RliqvYKpuslJFeUivf5QY6Ipi8yXfXn6TsRDbdxfGPi6oOR60Fa+4cJmCo6N5g57hBS6f2IdzQBNrZr7i1I/a3cFeK6XOc1G1tQaurx7Pu+qvACfJjLXKG66tHlaVhAHd/1l2FocgFNUDFFuKS3mnzt9hKys7sB4aO3O0OdohN/0NJC4ldV8/OmeXqqfkiPWcgPx3C8bYyXCX7QJNBHKrzbX1jW51Px7SIDWFDV6kxGwpQGGBMJg/k79gjjM+jhn4fg1/VP/Fx37mAnfLqpcTfiOkzSE80ORGefQ1XfGK/Dpa3ITrzRYW8xlR caos-dev-arm"
  inventory_template = "../ansible/inventory-template.tmpl"
  inventory_output   = var.inventory_output
  ansible_playbook   = "-e system_identity_client_id=${var.system_identity_client_id} -e nr_license_key=${var.license_key} -e repo_endpoint=${var.repository_endpoint} -e package_version=${var.package_version} -e fleet_id=${var.fleet_id} -e opamp_endpoint=${var.opamp_endpoint} -e token_endpoint=${var.token_endpoint} -e signature_validation_endpoint=${var.signature_validation_endpoint} -e otlp_endpoint=${var.otlp_endpoint} -e infra_staging=${local.infra_staging} ../ansible/install_ac_with_basic_config.yml"
  ec2_otels          = local.ec2_instances
}


variable "account_id" {
  description = "New Relic Account ID"
  type        = string
}
variable "api_key" {
  description = "New Relic API Key"
  type        = string
}

variable "slack_webhook_url" {
  description = "Slack Webhook URL where alerts notifications will be sent"
  type        = string
}

variable "nr_region" {
  description = "New Relic Region"
  type        = string
}

module "alerts" {
  source = "../../terraform/modules/nr_alerts"

  for_each = toset(local.hostnames)

  api_key           = var.api_key
  account_id        = var.account_id
  slack_webhook_url = var.slack_webhook_url

  policies_prefix = "Agent Control canaries metric monitoring"

  region      = var.nr_region
  instance_id = each.value
  conditions = [
    {
      name          = "CPU usage (percentage)"
      metric        = "max(cpuPercent) OR 0"
      sample        = "ProcessSample"
      threshold     = 0.06
      duration      = 3600
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name          = "CPU usage (percentage)"
      metric        = "max(cpuPercent) OR 0"
      sample        = "ProcessSample"
      threshold     = 0
      duration      = 3600
      operator      = "below_or_equals"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name          = "Memory usage (bytes)"
      metric        = "max(memoryResidentSizeBytes) OR 0"
      sample        = "ProcessSample"
      threshold     = 20000000
      duration      = 600
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name          = "Memory usage (bytes)"
      metric        = "max(memoryResidentSizeBytes) OR 0"
      sample        = "ProcessSample"
      threshold     = 0
      duration      = 600
      operator      = "below_or_equals"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name          = "Read bytes rate"
      metric        = "max(ioReadBytesPerSecond) OR 0"
      sample        = "ProcessSample"
      threshold     = 500000
      duration      = 300
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name          = "Written bytes rate"
      metric        = "max(ioWriteBytesPerSecond) OR 0"
      sample        = "ProcessSample"
      threshold     = 20000
      duration      = 300
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
  ]
}
