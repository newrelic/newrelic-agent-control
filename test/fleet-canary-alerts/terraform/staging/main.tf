# Fleet-level health alerting for the Agent Control canaries.
#
# The on-host and k8s canary alerts query per-instance telemetry (ProcessSample, K8sContainerSample,
# self-instrumentation Logs) in the canary *telemetry* account. Agent health, however, is reported
# by Fleet Control server-side as `AgentHeartbeat` events, which land in a SEPARATE *fleet-data*
# account (12213068 for staging). Those events carry a structured `agentHealthStatus` boolean keyed
# by `agentType` and `fleetGuid`, covering every managed agent across every canary fleet (on-host and
# k8s) at once — so a single fleet-wide alert replaces the per-instance, log-based "supervisor
# unhealthy" conditions. This root config owns that alert and is deployed once.

variable "account_id" {
  description = "New Relic fleet-data account ID that receives AgentHeartbeat events. This is NOT the canary telemetry account; for staging it is 12213068."
  type        = string
  default     = "12213068"
}

variable "api_key" {
  description = "New Relic User API key with write access to the fleet-data account."
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

  region      = "Staging"
  instance_id = "Agent_Control_Fleet_Health"

  conditions = [
    {
      # Fleet Control emits an `AgentHeartbeat` for every managed agent (~every 30s) with a structured
      # `agentHealthStatus` boolean, faceted by `agentType` and `fleetGuid`. This fires per agent-type
      # per fleet that stays unhealthy for 15 minutes straight (persistent, not a transient blip),
      # covering all canary fleets (on-host + k8s) from one refactor-proof structured signal — no
      # self-instrumentation and no Agent Control code change required.
      name               = "Agent Control agent unhealthy"
      threshold          = 0
      duration           = 900
      aggregation_window = 300
      operator           = "above"
      template_name      = "./alert_nrql_templates/agent_heartbeat_unhealthy.tftpl"
    },
  ]
}
