# Fleet-level health alerting for the Agent Control canaries (production).
#
# The on-host and k8s canary alerts query per-instance telemetry (ProcessSample, K8sContainerSample,
# self-instrumentation Logs) in the canary *telemetry* account. Agent health, however, is reported
# by Fleet Control server-side as `AgentHeartbeat` events, which land in a SEPARATE *fleet-data*
# account (6425865 for production). Those events carry a structured `agentHealthStatus` boolean keyed
# by `agentType` and `fleetGuid`, covering every managed agent across every canary fleet (on-host and
# k8s) at once — so a single fleet-wide alert replaces the per-instance, log-based "supervisor
# unhealthy" conditions. This root config owns that alert and is deployed once.
#
# The alert is created in the canary *telemetry* account (same account and key as the other canary
# alerts, where we have write access) and uses the NRQL condition's `data_account_id` to evaluate
# against the fleet-data account cross-account — so no write access to the fleet account is needed,
# only read. See `data_account_id` below.

variable "account_id" {
  description = "New Relic canary telemetry account ID the alert resources are created in (the production canary account, i.e. NEW_RELIC_PROD_ACCOUNT_ID). Write access required."
  type        = string
}

variable "data_account_id" {
  description = "New Relic fleet-data account ID that receives AgentHeartbeat events; the NRQL condition queries it cross-account (read access required). Passed in from CI via the FLEET_DATA_ACCOUNT_ID_PRODUCTION repo variable."
  type        = string
}

# CRITICAL: the production fleet-data account holds AgentHeartbeat for the ENTIRE customer base, so the
# alert MUST restrict to our canary fleets — a bare FACET would still evaluate every customer's agents
# and page us for their unhealthy agents.
variable "fleet_guids" {
  description = "Comma-separated canary fleet entity GUIDs to restrict the AgentHeartbeat query to (production). Passed in from CI via the ONHOST_FLEET_ID_PRODUCTION/ONHOST_WINDOWS_FLEET_ID_PRODUCTION/K8S_FLEET_ID_PRODUCTION repo variables — the single source of truth also used by component_onhost_canaries.yml and component_k8s_canaries.yml."
  type        = string
}

variable "api_key" {
  description = "New Relic User API key with write access to the telemetry account and read access to the fleet-data account."
  type        = string
}

variable "slack_webhook_url" {
  description = "Slack Webhook URL where alert notifications will be sent."
  type        = string
}

variable "emails" {
  description = "Comma-separated list of emails to receive alert notifications."
  type        = string
}

module "alerts" {
  source = "../../../terraform/modules/nr_alerts"

  api_key           = var.api_key
  account_id        = var.account_id
  slack_webhook_url = var.slack_webhook_url
  emails            = var.emails
  policies_prefix   = "Agent Control fleet health alerts"

  region         = "US"
  environment    = "production"
  instance_id    = "Agent_Control_Fleet_Health"
  alert_subtitle = "Agent Control — Fleet health canary"
  enable_email   = false

  # One issue (and one notification) per unhealthy agentType/fleetGuid, so each Slack message names the
  # specific offending agent. Combined with the 30-day violation time limit below, an agent notifies once
  # when it goes unhealthy, stays quiet while it remains unhealthy, and only re-notifies if it recovers
  # and then fails again (rather than re-notifying hourly).
  incident_preference          = "PER_CONDITION_AND_TARGET"
  violation_time_limit_seconds = 2592000

  conditions = [
    {
      # Fleet Control emits an `AgentHeartbeat` for every managed agent (~every 30s) with a structured
      # `agentHealthStatus` boolean, faceted by `agentType` and `fleetGuid`. This fires per agent-type
      # per fleet that stays unhealthy for 15 minutes straight (persistent, not a transient blip),
      # covering all canary fleets (on-host + k8s) from one refactor-proof structured signal — no
      # self-instrumentation and no Agent Control code change required.
      #
      # `data_account_id` makes the condition (created in this telemetry account) evaluate the query
      # against the fleet-data account where AgentHeartbeat actually lives.
      name               = "Agent Control agent unhealthy"
      threshold          = 0
      duration           = 900
      aggregation_window = 300
      operator           = "above"
      data_account_id    = var.data_account_id
      fleet_guids        = split(",", var.fleet_guids)
      template_name      = "./alert_nrql_templates/agent_heartbeat_unhealthy.tftpl"
    },
  ]
}
