resource "newrelic_alert_policy" "alert_policy_config" {
  name = format("%s: %s", var.region, var.instance_id)
}

locals {
  policies_with_instance_id = [
    for cond in var.conditions : {
      policy_id    = newrelic_alert_policy.alert_policy_config.id
      instance_id = var.instance_id
      condition    = cond
    }
  ]
}

resource "newrelic_workflow" workflow {
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
    channel_id = newrelic_notification_channel.channel.id
  }
}


resource newrelic_notification_channel channel {
  name = var.instance_id
  type = "WEBHOOK"
  destination_id = newrelic_notification_destination.destination.id
  product        = "IINT"

  property {
    key = "payload"
    value = "{\"text\": \":warning: ${var.instance_id} Alert @hero\"}"
  }
}

resource "newrelic_notification_destination" "destination" {
  name = "SlackWebhook"
  type = "WEBHOOK"

  property {
    key = "url"
    value = var.slack_webhook_url
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
  violation_time_limit_seconds = 3600

  nrql {
    query = templatefile(
      local.policies_with_instance_id[count.index].condition.template_name,
      merge(
        {
          "instance_id" : "${local.policies_with_instance_id[count.index].instance_id}",
          "function" : null,
          "wheres" : {}
        },
        local.policies_with_instance_id[count.index].condition
      )
    )
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
