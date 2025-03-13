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
  assembled_ec2 = var.ec2_prefix == "" ? local.ec2_configs : { for k, v in local.ec2_configs : format("%s%s%s", var.ec2_prefix, ":", k) => v }

  hostnames = [for k, v in local.assembled_ec2 : v.metrics_attributes_id ]
}

module "ec2_instances" {
  source  = "registry.terraform.io/terraform-aws-modules/ec2-instance/aws"

  for_each               = local.assembled_ec2

  name                   = each.key
  ami                    = each.value.ami
  instance_type          = each.value.instance_type
  key_name               = each.value.key_name
  subnet_id              = each.value.subnet
  vpc_security_group_ids = each.value.security_groups
}

resource "null_resource" "wait_linux" {

  for_each = local.assembled_ec2
  provisioner "remote-exec" {
    connection {
      type        = "ssh"
      user        = each.value.username
      host        = module.ec2_instances[each.key].private_ip
      private_key = file("~/.ssh/caos-dev-arm.cer")
    }

    inline = [
      "echo 'connected'"
    ]
  }
}

resource "local_file" "AnsibleInventory" {
  depends_on = [null_resource.wait_linux]

  content = templatefile("../ansible/inventory-template.tmpl",
    {
      host-ids          = [for k, p in module.ec2_instances : k],
      host-user         = [for k, p in module.ec2_instances : local.assembled_ec2[k].username],
      host-private-ip   = [for k, p in module.ec2_instances : p.private_ip],
      host-unique-id = [for k, p in module.ec2_instances : local.assembled_ec2[k].metrics_attributes_id],
    }
  )
  filename = var.inventory_output
}

resource "null_resource" "ansible" {
  depends_on = [local_file.AnsibleInventory]

  triggers = {
    always_run = "${timestamp()}"
  }

  provisioner "local-exec" {
    command = "ANSIBLE_HOST_KEY_CHECKING=False ansible-playbook -i ${var.inventory_output} -e nr_license_key=${var.license_key} -e repo_endpoint=${var.repository_endpoint} --private-key ~/.ssh/caos-dev-arm.cer ../ansible/install_ac_with_basic_config.yml"
  }
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