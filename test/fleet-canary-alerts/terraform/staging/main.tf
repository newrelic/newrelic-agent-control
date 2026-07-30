# Fleet-level health alerting for the Agent Control canaries.
#
# The on-host and k8s canary alerts query per-instance telemetry (ProcessSample, K8sContainerSample,
# self-instrumentation Logs) in the canary *telemetry* account. Agent health, however, is reported
# by Fleet Control server-side as `AgentHeartbeat` events, which land in a SEPARATE *fleet-data*
# account (12213068 for staging). Those events carry a structured `agentHealthStatus` boolean keyed
# by `agentType` and `fleetGuid`, covering every managed agent across every canary fleet (on-host and
# k8s) at once — so a single fleet-wide alert replaces the per-instance, log-based "supervisor
# unhealthy" conditions. This root config owns that alert and is deployed once.
#
# The alert is created in the canary *telemetry* account (same account and key as the other canary
# alerts, where we have write access) and uses the NRQL condition's `data_account_id` to evaluate
# against the fleet-data account cross-account — so no write access to the fleet account is needed,
# only read (which is available). See `data_account_id` below.

variable "account_id" {
  description = "New Relic canary telemetry account ID the alert resources are created in (same as the other canary alerts; staging = 12213067). Write access required."
  type        = string
  default     = "12213067"
}

variable "data_account_id" {
  description = "New Relic fleet-data account ID that receives AgentHeartbeat events; the NRQL condition queries it cross-account. Staging = 12213068 (read access required)."
  type        = string
  default     = "12213068"
}

# The fleet-data account holds AgentHeartbeat for MANY fleets (in production, the whole customer base),
# so the alert MUST restrict to our canary fleets — a bare FACET would group by fleet but still evaluate
# every customer's agents. These are the staging canary fleet entity GUIDs, i.e. the FLEET_ID_STAGING /
# WINDOWS_FLEET_ID_STAGING values the canaries are provisioned with. Keep in sync with
# component_onhost_canaries.yml and component_k8s_canaries.yml (the source of truth for FLEET_IDs).
variable "fleet_guids" {
  description = "Canary fleet entity GUIDs to restrict the AgentHeartbeat query to (staging)."
  type        = list(string)
  default = [
    "MTIyMTMwNjh8TkdFUHxGTEVFVHwwMTlhZTNiNS01Yjg5LTdkNjYtYWU0MC1lNmZkOTY2ZDFhMDA", # host-canaries-staging (on-host Linux)
    "MTIyMTMwNjh8TkdFUHxGTEVFVHwwMTljMjNjYy0yM2I2LTc1OTYtODBkNy0yMzAzYWIyYzcwMTA", # agent_control_canaries (on-host Windows)
    "MTIyMTMwNjh8TkdFUHxGTEVFVHwwMTk1ZDE1NC1iNTI0LTdhMTYtYWExYS0wYzQ2Y2VhOGFiMzg", # k8s-canaries-staging-1
  ]
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

  region         = "Staging"
  environment    = "staging"
  instance_id    = "Agent_Control_Fleet_Health"
  alert_subtitle = "Agent Control — Fleet health canary"

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
      fleet_guids        = var.fleet_guids
      template_name      = "./alert_nrql_templates/agent_heartbeat_unhealthy.tftpl"
    },
  ]
}
