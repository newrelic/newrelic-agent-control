resource "newrelic_alert_policy" "alert_k8s_canary" {
  name = format("%s: %s", var.region, var.cluster_name)
}

locals {
  policies_with_cluster_name = [
    for pol in var.conditions : {
      policy_id    = newrelic_alert_policy.alert_k8s_canary.id
      cluster_name = var.cluster_name
      condition    = pol
    }
  ]
}

# Uncomment this to "debug" the generated structure
#output prueba {
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
