resource "newrelic_alert_policy" "alert_k8s_canary" {
  name = format("%s: %s", var.region, var.cluster_name)
}

locals {
  policies_with_cluster_name = [
    for cond in var.conditions : {
      policy_id    = newrelic_alert_policy.alert_k8s_canary.id
      cluster_name = var.cluster_name
      condition    = cond
    }
  ]
}

resource "newrelic_workflow" workflow {
  name                  = var.cluster_name
  muting_rules_handling = "NOTIFY_ALL_ISSUES"

  issues_filter {
    name = "Issue Filter"
    type = "FILTER"
    predicate {
      attribute = "labels.policyIds"
      operator  = "EXACTLY_MATCHES"
      values    = [newrelic_alert_policy.alert_k8s_canary.id]
    }
  }

  destination {
    channel_id = newrelic_notification_channel.channel.id
  }
}


resource newrelic_notification_channel channel {
  name = var.cluster_name
  type = "WEBHOOK"
  destination_id = newrelic_notification_destination.destination.id
  product        = "IINT"

  property {
    key = "payload"
    value = "{\"text\": \":warning: ${var.cluster_name} Alert @hero\"}"
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
  count = length(local.policies_with_cluster_name)

  account_id                   = var.account_id
  policy_id                    = local.policies_with_cluster_name[count.index].policy_id
  name                         = local.policies_with_cluster_name[count.index].condition.name
  violation_time_limit_seconds = 3600

  nrql {
    query = templatefile(
      local.policies_with_cluster_name[count.index].condition.template_name,
      merge(
        {
          "cluster_name" : "${local.policies_with_cluster_name[count.index].cluster_name}",
          "function" : null,
          "wheres" : {}
        },
        local.policies_with_cluster_name[count.index].condition
      )
    )
  }

  critical {
    operator              = local.policies_with_cluster_name[count.index].condition.operator
    threshold             = local.policies_with_cluster_name[count.index].condition.threshold
    threshold_duration    = local.policies_with_cluster_name[count.index].condition.duration
    threshold_occurrences = "ALL"
  }
}

output "queries" {
  value = [newrelic_nrql_alert_condition.condition_nrql_canary.*.nrql]
}
