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

variable "policies_prefix" {
  default = ""
}

variable "slack_webhook_url" {
  description = "Slack webhook where New Relic will send alerts"
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
