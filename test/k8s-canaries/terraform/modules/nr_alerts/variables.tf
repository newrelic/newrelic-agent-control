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

variable "cluster_name" {
  default = "Agent_Control_Canaries_Staging-Cluster"
}

variable "policies_prefix" {
  default = "[Staging] Agent Control canaries metric monitoring"
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
#   template_name = "generic_metric_comparator"
# },
# {
#   name = "System / Cpu IOWait Percent"
#   metric = "cpuIOWaitPercent"
#   sample = "SystemSample"
#   threshold = 0.5 # max 0.112 in last week
#   duration = 600
#   operator = "above"
#   template_name = "generic_metric_comparator"
# },
# ...
# ]
#
variable "conditions" {
  default = []
}
