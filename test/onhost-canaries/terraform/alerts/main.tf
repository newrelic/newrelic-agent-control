variable "nr_region" {
  description = "New Relic Region"
  type        = string
  validation {
    condition     = can(regex("^(US|EU|Staging)$", var.nr_region))
    error_message = "Unsupported region"
  }
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

variable "infra_backend_config" {
  description = "Path to the infra backend config file"
  type        = string
}

# Read outputs from infra module via remote state
data "terraform_remote_state" "infra" {
  backend = "s3"

  config = {
    bucket = "agent-control-terraform-states"
    key    = var.infra_backend_config
    region = "us-east-2"
  }
}

locals {
  # Read from infra remote state
  ec2_prefix    = data.terraform_remote_state.infra.outputs.ec2_prefix
  ec2_instances = data.terraform_remote_state.infra.outputs.ec2_instances

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
    "${local.ec2_prefix}-${replace(k, "/[:._]/", "-")}" => {
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
