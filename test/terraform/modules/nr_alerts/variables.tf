# NR Account ID
variable "account_id" {
  default = ""
}

# NR User Api Key
variable "api_key" {
  default = ""
}

# US/EU/Staging
variable "region" {
  default = "US"

  validation {
    condition     = can(regex("^(US|EU|Staging)$", var.region))
    error_message = "Unsupported region"
  }
}

variable "instance_id" {
  description = "Identifier for the instances that will be monitored (i.e. cluster name for k8s and hostname for on-host)"
}

# Human-readable environment label shown in notifications (e.g. "staging" / "production"). Defaults to
# `region` when unset so existing configs keep their current behavior.
variable "environment" {
  default = ""
}

# Short label shown at the top of the Slack notification identifying the kind of canary alert
# (e.g. "Agent Control — Fleet health canary"). Defaults to a generic canary label.
variable "alert_subtitle" {
  default = "Agent Control canary alert"
}

# Alert policy issue grouping. PER_POLICY groups every violation into one issue; PER_CONDITION_AND_TARGET
# opens a separate issue per faceted target (e.g. per agentType/fleetGuid) so each notification names the
# specific offender.
variable "incident_preference" {
  default = "PER_POLICY"

  validation {
    condition     = can(regex("^(PER_POLICY|PER_CONDITION|PER_CONDITION_AND_TARGET)$", var.incident_preference))
    error_message = "incident_preference must be PER_POLICY, PER_CONDITION or PER_CONDITION_AND_TARGET"
  }
}

# How long an incident stays open before auto-closing. The default (1h) makes a still-breaching incident
# recycle hourly (re-notifying); set high (max 2592000 = 30d) so the incident only closes on real recovery.
variable "violation_time_limit_seconds" {
  default = 3600
}

variable "policies_prefix" {
  default = ""
}

variable "slack_webhook_url" {
  description = "Slack webhook where New Relic will send alerts"
}

variable "emails" {
  description = "Comma-separated list of emails to receive alert notifications"
}

# conditions should follow next structure:
#[
# {
#   name          = "System / Core Count"
#   metric        = "coreCount"
#   sample        = "SystemSample"
#   threshold     = 0
#   duration      = 600
#   operator      = "above"
#   template_name = "./generic_metrics_threshold.tfpl"
# },
# {
#   name = "System / Cpu IOWait Percent"
#   metric = "cpuIOWaitPercent"
#   sample = "SystemSample"
#   threshold = 0.5 # max 0.112 in last week
#   duration = 600
#   operator = "above"
#   template_name = "./generic_metrics_threshold.tfpl"
# },
# ...
# ]
#
variable "conditions" {
  default = []
}
