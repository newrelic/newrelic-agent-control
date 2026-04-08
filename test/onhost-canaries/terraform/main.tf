variable "ec2_prefix" {
  description = "Prefix for EC2 instances"
  type        = string
}

variable "inventory_output" {
  description = "Path to write the inventory file"
  type        = string
}

variable "windows_password" {
  description = "Windows AMI password for WinRM connection"
  type        = string
}

variable "nr_region" {
  description = "New Relic Region"
  type        = string
  validation {
    condition     = can(regex("^(US|EU|Staging)$", var.nr_region))
    error_message = "Unsupported region"
  }
}

variable "pvt_key_path" {
  description = "Path to SSH private key"
  type        = string
  default     = "~/.ssh/caos-dev-arm.cer"
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
    }
    "amd64:windows_2022" = {
      ami             = "ami-04382be054853bd1f"
      subnet          = "subnet-00aa02e6d991b478e"
      security_groups = ["sg-04ae18f8c34a11d38"]
      key_name        = "caos-dev-arm"
      instance_type   = "t3a.small"
      username        = "Administrator"
      platform        = "windows"
      python          = ""
    }
  }

  # Append ec2_prefix to ec2 instances name
  assembled_ec2 = {
    for k, v in local.ec2_instances :
    format("%s-%s", var.ec2_prefix, replace(k, "/[:._]/", "-")) => v
  }

  # ============================================================================
  # Alert Configuration
  # ============================================================================

  // Conditions shared across all platforms
  common_alert_conditions = [
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
    {
      name          = "Agent Control metrics presence"
      metric        = "count(*)"
      sample        = "ProcessSample"
      threshold     = 0
      duration      = 3600
      operator      = "below_or_equals"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
  ]

  // Platform-specific memory conditions.
  // Linux uses virtual size; Windows uses working set (physical memory committed to the process).
  memory_alert_condition_by_platform = {
    linux = {
      name          = "Memory usage (bytes)"
      metric        = "max(memoryResidentSizeBytes) OR 0"
      sample        = "ProcessSample"
      threshold     = 42000000
      duration      = 600
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    }
    windows = {
      name          = "Memory usage (bytes)"
      # For the purpose of leak detection using memoryVirtualSizeBytes reflects better the AC memory intent of usage,
      # as memoryResidentSizeBytes gets heavily affected by the way windows manages memory.
      metric        = "max(memoryVirtualSizeBytes) OR 0"
      sample        = "ProcessSample"
      threshold     = 35000000
      duration      = 600
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
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
  instance_alerts = {
    for k, v in local.ec2_instances :
    "${var.ec2_prefix}-${replace(k, "/[:._]/", "-")}" => {
      platform = v.platform
      conditions = concat(
        local.common_alert_conditions,
        [local.memory_alert_condition_by_platform[v.platform]]
      )
    }
  }

  // Flatten the structure for creating individual alert conditions
  alert_conditions_flat = flatten([
    for instance_id, instance_data in local.instance_alerts : [
      for idx, condition in instance_data.conditions : {
        instance_id  = instance_id
        condition    = condition
        policy_id    = newrelic_alert_policy.alert_policy[instance_id].id
        unique_key   = "${instance_id}-${idx}"
      }
    ]
  ])
}

# Create EC2 instances
module "ec2_instances" {
  source  = "registry.terraform.io/terraform-aws-modules/ec2-instance/aws"
  version = "3.4.0"

  for_each = local.assembled_ec2

  name                   = each.key
  ami                    = each.value.ami
  instance_type          = each.value.instance_type
  key_name               = each.value.key_name
  subnet_id              = each.value.subnet
  vpc_security_group_ids = each.value.security_groups
}

# Wait for Linux instances to be ready
resource "null_resource" "wait_linux" {
  for_each = {
    for key, val in local.assembled_ec2 :
    key => val if val.platform == "linux"
  }

  provisioner "remote-exec" {
    connection {
      type        = "ssh"
      user        = each.value.username
      host        = module.ec2_instances[each.key].private_ip
      private_key = file(var.pvt_key_path)
    }

    inline = [
      "echo 'connected'"
    ]
  }

  depends_on = [module.ec2_instances]
}

# Wait for Windows instances to be ready
resource "null_resource" "wait_windows" {
  for_each = {
    for key, val in local.assembled_ec2 :
    key => val if val.platform == "windows"
  }

  provisioner "remote-exec" {
    connection {
      type     = "winrm"
      user     = each.value.username
      host     = module.ec2_instances[each.key].private_ip
      password = var.windows_password
      insecure = true
      https    = true
    }

    inline = [
      "echo 'connected'"
    ]
  }

  depends_on = [module.ec2_instances]
}

# Generate Ansible inventory file
resource "local_file" "ansible_inventory" {
  depends_on = [null_resource.wait_linux, null_resource.wait_windows]

  content = templatefile("${path.module}/../ansible/inventory-template.tmpl",
    {
      gateway-ids        = []
      gateway-user       = []
      gateway-private-ip = []
      agent-ids          = [for k, p in module.ec2_instances : k]
      agent-python       = [for k, p in module.ec2_instances : local.assembled_ec2[k].python]
      agent-user         = [for k, p in module.ec2_instances : local.assembled_ec2[k].username]
      agent-private-ip   = [for k, p in module.ec2_instances : p.private_ip]
      instance-id        = [for k, p in module.ec2_instances : p.id]
      platform           = [for k, p in module.ec2_instances : local.assembled_ec2[k].platform]
      windows_password   = var.windows_password
    }
  )
  filename = var.inventory_output
}

# Outputs
output "ec2_instances" {
  description = "EC2 instances configuration"
  value       = local.ec2_instances
}

output "ec2_prefix" {
  description = "Prefix for EC2 instances"
  value       = var.ec2_prefix
}

output "instance_ids" {
  description = "EC2 instance IDs"
  value = {
    for k, v in module.ec2_instances : k => v.id
  }
}

output "instance_private_ips" {
  description = "EC2 instance private IPs"
  value = {
    for k, v in module.ec2_instances : k => v.private_ip
  }
}

output "inventory_file" {
  description = "Path to generated Ansible inventory file"
  value       = local_file.ansible_inventory.filename
}

output "ansible_inventory_content" {
  description = "Ansible inventory file content (generated from Terraform state)"
  value       = local_file.ansible_inventory.content
  sensitive   = true
}


# Create alert policy for each instance
resource "newrelic_alert_policy" "alert_policy" {
  for_each = local.instance_alerts

  name = format("%s: %s", var.nr_region, each.key)
}

# Create a single notification destination for all instances
resource "newrelic_notification_destination" "slack_webhook" {
  name = "SlackWebhook"
  type = "WEBHOOK"

  property {
    key   = "url"
    value = var.slack_webhook_url
  }
}

# Create notification channel for each instance
resource "newrelic_notification_channel" "channel" {
  for_each = local.instance_alerts

  name           = each.key
  type           = "WEBHOOK"
  destination_id = newrelic_notification_destination.slack_webhook.id
  product        = "IINT"

  property {
    key   = "payload"
    value = "{\"text\": \":warning: ${each.key} Alert @hero\"}"
  }
}

# Create workflow for each instance
resource "newrelic_workflow" "workflow" {
  for_each = local.instance_alerts

  name                  = each.key
  muting_rules_handling = "NOTIFY_ALL_ISSUES"

  issues_filter {
    name = "Issue Filter"
    type = "FILTER"
    predicate {
      attribute = "labels.policyIds"
      operator  = "EXACTLY_MATCHES"
      values    = [newrelic_alert_policy.alert_policy[each.key].id]
    }
  }

  destination {
    channel_id = newrelic_notification_channel.channel[each.key].id
  }
}

# Create NRQL alert conditions
resource "newrelic_nrql_alert_condition" "condition" {
  for_each = { for item in local.alert_conditions_flat : item.unique_key => item }

  account_id                   = var.account_id
  policy_id                    = each.value.policy_id
  name                         = each.value.condition.name
  violation_time_limit_seconds = 3600

  nrql {
    query = templatefile(
      each.value.condition.template_name,
      merge(
        {
          "instance_id" : each.value.instance_id,
          "function" : null,
          "wheres" : {}
        },
        each.value.condition
      )
    )
  }

  critical {
    operator              = each.value.condition.operator
    threshold             = each.value.condition.threshold
    threshold_duration    = each.value.condition.duration
    threshold_occurrences = "ALL"
  }
}

output "alert_policies" {
  description = "Created alert policies"
  value = {
    for k, v in newrelic_alert_policy.alert_policy : k => {
      id   = v.id
      name = v.name
    }
  }
}

output "alert_conditions" {
  description = "Created alert conditions"
  value = {
    for k, v in newrelic_nrql_alert_condition.condition : k => {
      name  = v.name
      query = v.nrql[0].query
    }
  }
}
