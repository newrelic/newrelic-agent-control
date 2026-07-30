resource "newrelic_alert_policy" "alert_policy_config" {
  name                = format("%s: %s", var.region, var.instance_id)
  incident_preference = var.incident_preference
}

locals {
  # Environment label for notifications; falls back to `region` when not explicitly set.
  environment = var.environment != "" ? var.environment : var.region

  policies_with_instance_id = [
    for cond in var.conditions : {
      policy_id   = newrelic_alert_policy.alert_policy_config.id
      instance_id = var.instance_id
      condition   = cond
    }
  ]
}

resource "newrelic_workflow" "workflow" {
  name                  = var.instance_id
  muting_rules_handling = "NOTIFY_ALL_ISSUES"

  issues_filter {
    name = "Issue Filter"
    type = "FILTER"
    predicate {
      attribute = "labels.policyIds"
      operator  = "EXACTLY_MATCHES"
      values    = [newrelic_alert_policy.alert_policy_config.id]
    }
  }

  destination {
    channel_id = newrelic_notification_channel.slack_channel.id
  }

  destination {
    channel_id = newrelic_notification_channel.email_channel.id
  }
}


resource "newrelic_notification_channel" "slack_channel" {
  name           = var.instance_id
  type           = "WEBHOOK"
  destination_id = newrelic_notification_destination.slack_webhook.id
  product        = "IINT"

  property {
    key   = "payload"
    value = templatefile("${path.module}/alert_slack_payload.tftpl", {
      instance_id    = var.instance_id
      environment    = local.environment
      alert_subtitle = var.alert_subtitle
    })
  }
}

resource "newrelic_notification_channel" "email_channel" {
  name           = var.instance_id
  type           = "EMAIL"
  destination_id = newrelic_notification_destination.email.id
  product        = "IINT"

  property {
    key   = "subject"
    value = "Alert: ${var.instance_id}"
  }
}

resource "newrelic_notification_destination" "slack_webhook" {
  name = "SlackWebhook"
  type = "WEBHOOK"

  property {
    key   = "url"
    value = var.slack_webhook_url
  }
}

resource "newrelic_notification_destination" "email" {
  name = "Email"
  type = "EMAIL"

  property {
    key   = "email"
    value = var.emails
  }
}

# Uncomment this to "debug" the generated structure
#output test {
#  value = local.policies_with_display_names
#}

resource "newrelic_nrql_alert_condition" "condition_nrql_canary" {
  count = length(local.policies_with_instance_id)

  account_id                   = var.account_id
  policy_id                    = local.policies_with_instance_id[count.index].policy_id
  name                         = local.policies_with_instance_id[count.index].condition.name
  violation_time_limit_seconds = try(local.policies_with_instance_id[count.index].condition.violation_time_limit_seconds, var.violation_time_limit_seconds)

  # Defaults values from https://registry.terraform.io/providers/newrelic/newrelic/latest/docs/resources/nrql_alert_condition#example-usage
  aggregation_window = try(local.policies_with_instance_id[count.index].condition.aggregation_window, 60)
  slide_by           = try(local.policies_with_instance_id[count.index].condition.slide_by, 30)


  nrql {
    query = templatefile(
      local.policies_with_instance_id[count.index].condition.template_name,
      merge(
        {
          "instance_id" : "${local.policies_with_instance_id[count.index].instance_id}",
          "function" : null,
          "wheres" : {},
        },
        local.policies_with_instance_id[count.index].condition
      )
    )
    # Optional cross-account query: evaluate the NRQL against a different account than the one the
    # condition lives in. Used by the fleet alerts, whose AgentHeartbeat data lives in the fleet
    # account while the condition is created in the (writable) canary telemetry account. Defaults to
    # the condition's own account when unset.
    data_account_id = try(local.policies_with_instance_id[count.index].condition.data_account_id, null)
  }

  critical {
    operator              = local.policies_with_instance_id[count.index].condition.operator
    threshold             = local.policies_with_instance_id[count.index].condition.threshold
    threshold_duration    = local.policies_with_instance_id[count.index].condition.duration
    threshold_occurrences = "ALL"
  }
}

output "queries" {
  value = [newrelic_nrql_alert_condition.condition_nrql_canary.*.nrql]
}
