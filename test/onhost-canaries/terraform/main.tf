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

variable "emails" {
  description = "Comma-separated list of emails to receive alert notifications"
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
      metric        = "cpuPercent"
      threshold     = 0.06
      duration      = 3600
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name          = "Read bytes rate"
      metric        = "ioReadBytesPerSecond"
      threshold     = 500000
      duration      = 300
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name          = "Written bytes rate"
      metric        = "ioWriteBytesPerSecond"
      threshold     = 20000
      duration      = 300
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name          = "Agent Control metrics presence"
      metric        = "*"
      threshold     = 0
      duration      = 3600
      operator      = "below_or_equals"
      template_name = "./alert_nrql_templates/generic_metric_count.tftpl"
    },
    {
      # Fires if no self-instrumentation logs are received in a 10-minute window,
      # which indicates AC has stopped emitting OTel logs (crash, misconfiguration, etc.).
      name               = "Self-instrumentation logs presence"
      threshold          = 0
      duration           = 600
      aggregation_window = 600
      operator           = "below_or_equals"
      template_name      = "./alert_nrql_templates/log_presence.tftpl"
    },
    {
      # Distinct tripwire for AC-internal hard errors (panics, config/OpAMP failures) that surface as
      # ERROR-level self-instrumentation logs but do not necessarily flip a sub-agent to unhealthy.
      name               = "Agent Control error logs"
      threshold          = 0
      duration           = 1800
      aggregation_window = 600
      operator           = "above"
      template_name      = "./alert_nrql_templates/log_error_presence.tftpl"
    },
  ]

  // Platform-specific memory conditions.
  // Linux uses virtual size; Windows uses working set (physical memory committed to the process).
  memory_alert_condition_by_platform = {
    linux = [
      {
        name          = "Memory usage (bytes)"
        metric        = "memoryResidentSizeBytes"
        threshold     = 42000000
        duration      = 600
        operator      = "above"
        template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
      },
      {
        # This alert should detect slow memory leaks.
        #
        # For that, we compute the slope of the line (derivative function), with 3 hours of data (aggregation_window).
        # We then smooth the curve by computing the slope every hour (slide_by) and check that the slope is
        # above 210KB/hour (threshold) for at least 6 hours (duration).
        #
        # That roughly translates to +5MB over 24 hours. False positives should be unlikely with the current threshold,
        # but we can adjust it.
        #
        # Bare in mind that we are using 3 hour windows. The duration must be computed as the multiplication of the
        # aggregation_window by the number of data points we want to be above the threshold to trigger the alert.
        # In our case, we want 2 data points to be above the threshold, so the duration is 3 hours * 2 = 6 hours.
        name               = "Memory growth (bytes/hour)"
        metric             = "derivative(memoryResidentSizeBytes, 1 hour)"
        aggregation_window = 10800
        slide_by           = 3600
        threshold          = 210000
        duration           = 21600
        operator           = "above"
        template_name      = "./alert_nrql_templates/generic_metric_derivative.tftpl"
      }
    ],
    windows = [
      {
        name = "Memory usage (bytes)"
        # For the purpose of leak detection using memoryVirtualSizeBytes reflects better the AC memory intent of usage,
        # as memoryResidentSizeBytes gets heavily affected by the way windows manages memory.
        metric        = "memoryVirtualSizeBytes"
        threshold     = 35000000
        duration      = 600
        operator      = "above"
        template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
      },
      {
        # This alert should detect slow memory leaks.
        #
        # For that, we compute the slope of the line (derivative function), with 3 hours of data (aggregation_window).
        # We then smooth the curve by computing the slope every hour (slide_by) and check that the slope is
        # above 210KB/hour (threshold) for at least 6 hours (duration).
        #
        # That roughly translates to +5MB over 24 hours. False positives should be unlikely with the current threshold,
        # but we can adjust it.
        #
        # Bare in mind that we are using 3 hour windows. The duration must be computed as the multiplication of the
        # aggregation_window by the number of data points we want to be above the threshold to trigger the alert.
        # In our case, we want 2 data points to be above the threshold, so the duration is 3 hours * 2 = 6 hours.
        name = "Memory growth (bytes/hour)"
        # For the purpose of leak detection using memoryVirtualSizeBytes reflects better the AC memory intent of usage,
        # as memoryResidentSizeBytes gets heavily affected by the way windows manages memory.
        metric             = "derivative(memoryVirtualSizeBytes, 1 hour)"
        aggregation_window = 10800
        slide_by           = 3600
        threshold          = 210000
        duration           = 21600
        operator           = "above"
        template_name      = "./alert_nrql_templates/generic_metric_derivative.tftpl"
      }
    ]
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
        local.memory_alert_condition_by_platform[v.platform]
      )
    }
  }
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

# Create alert policy, workflow, notification channels and NRQL conditions for each instance via the shared module
module "alerts" {
  source = "../../terraform/modules/nr_alerts"

  for_each = local.instance_alerts

  api_key           = var.api_key
  account_id        = var.account_id
  slack_webhook_url = var.slack_webhook_url
  emails            = var.emails

  region      = var.nr_region
  instance_id = each.key

  conditions = each.value.conditions
}
