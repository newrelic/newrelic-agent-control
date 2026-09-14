# Use the EKS cluster module
module "eks_cluster" {
  source               = "../modules/eks_cluster"
  canary_name          = "Agent_Control_Canaries_Production"
  cluster_desired_size = 2
  cluster_max_size     = 3
  cluster_min_size     = 2
}

variable "account_id" {}
variable "api_key" {}
variable "slack_webhook_url" {}
variable "emails" {}
module "alerts" {
  source = "../../../terraform/modules/nr_alerts"

  api_key           = var.api_key
  account_id        = var.account_id
  slack_webhook_url = var.slack_webhook_url
  emails            = var.emails
  policies_prefix   = "Agent Control canaries metric monitoring"

  region      = "US"
  instance_id = "Agent_Control_Canaries_Production-Cluster"

  conditions = [
      {
      name                           = "K8sContainerSample metric presence"
      metric                         = "*"
      sample                         = "K8sContainerSample"
      threshold                      = 0
      duration                       = 600
      operator                       = "below_or_equals"
      template_name                  = "./alert_nrql_templates/generic_metric_count.tftpl"
      # Loss-of-signal config. Be aware that lost of signal is only detected if there where previous 
      # signals flowing.
      expiration_duration            = 300
      open_violation_on_expiration   = true
      close_violations_on_expiration = false
      ignore_on_expected_termination = false
    },
    {
      name          = "CPU usage (cores)"
      metric        = "cpuUsedCores"
      sample        = "K8sContainerSample"
      threshold     = 0.06 # +50% of observed value https://staging.onenr.io/0VRVAJrmJwa
      duration      = 3600
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_max.tftpl"
    },
    {
      name          = "Memory usage (bytes)"
      metric        = "memoryWorkingSetBytes"
      sample        = "K8sContainerSample"
      threshold     = 16000000 # +25% of observed value https://staging.onenr.io/0dQeV0JdVwe
      duration      = 600
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_max.tftpl"
    },
    {
      name          = "Storage usage (bytes)"
      metric        = "fsUsedBytes"
      sample        = "K8sContainerSample"
      threshold     = 10000 # 10 KB
      duration      = 3600
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_max.tftpl"
    },
    {
      # Fires if no self-instrumentation logs are received in a 10-minute window.
      name               = "Self-instrumentation logs presence"
      threshold          = 0
      duration           = 600
      aggregation_window = 600
      operator           = "below_or_equals"
      template_name      = "./alert_nrql_templates/log_presence.tftpl"
      # Loss-of-signal config. Be aware that lost of signal is only detected if there where previous 
      # signals flowing.
      expiration_duration            = 300
      open_violation_on_expiration   = true
      close_violations_on_expiration = false
      ignore_on_expected_termination = false
    },
    {
      # Distinct tripwire for AC-internal hard errors (panics, config/OpAMP failures) that surface as
      # ERROR-level self-instrumentation logs but do not necessarily flip a sub-agent to unhealthy.
      name               = "Agent Control error logs"
      threshold          = 0
      duration           = 1800
      aggregation_window = 600
      operator           = "above"
      template_name      = "./alert_nrql_templates/log_error_presence.tftpl"
    },
  ]
}
