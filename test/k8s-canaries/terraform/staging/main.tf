# Use the EKS cluster module
module "eks_cluster" {
  source               = "../modules/eks_cluster"
  canary_name          = "Agent_Control_Canaries_Staging"
  cluster_desired_size = 2
  cluster_max_size     = 3
  cluster_min_size     = 2
}


variable "account_id" {}
variable "api_key" {}
variable "slack_webhook_url" {}

module "alerts" {
  source = "../../../terraform/modules/nr_alerts"

  api_key           = var.api_key
  account_id        = var.account_id
  slack_webhook_url = var.slack_webhook_url
  policies_prefix   = "Agent Control canaries metric monitoring"

  region      = "Staging"
  instance_id = "Agent_Control_Canaries_Staging-Cluster"

  conditions = [
    {
      name          = "CPU usage (cores)"
      metric        = "cpuUsedCores"
      sample        = "K8sContainerSample"
      threshold     = 0.06 # +50% of observed value https://staging.onenr.io/0VRVAJrmJwa
      duration      = 3600
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name          = "Memory usage (bytes)"
      metric        = "memoryWorkingSetBytes"
      sample        = "K8sContainerSample"
      threshold     = 16000000 # +25% of observed value https://staging.onenr.io/0dQeV0JdVwe
      duration      = 600
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name          = "Storage usage (bytes)"
      metric        = "fsUsedBytes"
      sample        = "K8sContainerSample"
      threshold     = 10000 # 10 KB
      duration      = 3600
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    # Trigger alert if no metrics
    {
      name          = "CPU usage (cores)"
      metric        = "cpuUsedCores"
      sample        = "K8sContainerSample"
      threshold     = 0
      duration      = 3600
      operator      = "below_or_equals"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name          = "Memory usage (bytes)"
      metric        = "memoryWorkingSetBytes"
      sample        = "K8sContainerSample"
      threshold     = 0
      duration      = 600
      operator      = "below_or_equals"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      # This alert should detect slow memory leaks.
      #
      # For that, we compute the slope of the line (derivative function), with 3 hours of data (aggregation_window).
      # We then smooth the curve by computing the slope every hour (slide_by) and check that the slope is
      # above 210KB/hour (threshold) for at least 6 hours (duration).
      #
      # That roughly translates to +5MB over 24 hours. False positives should be unlikely with the current threshold,
      # but we can adjust it.
      #
      # Bare in mind that we are using 3 hour windows. The duration must be computed as the multiplication of the
      # aggregation_window by the number of data points we want to be above the threshold to trigger the alert.
      # In our case, we want 2 data points to be above the threshold, so the duration is 3 hours * 2 = 6 hours.
      name               = "Memory growth (bytes/hour)"
      metric             = "derivative(memoryResidentSizeBytes, 1 hour)"
      sample             = "ProcessSample"
      aggregation_window = 10800
      slide_by           = 3600
      threshold          = 210000
      duration           = 21600
      operator           = "above"
      template_name      = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name          = "Agent Control container"
      metric        = "*"
      sample        = "K8sContainerSample"
      threshold     = 0
      duration      = 600
      operator      = "below_or_equals"
      template_name = "./alert_nrql_templates/generic_metric_count.tftpl"
    },
    {
      name      = "Opamp traces per minute"
      metric    = "*"
      sample    = "Span"
      threshold = 1
      duration  = 3600
      operator  = "below_or_equals"
      wheres = {
        name = "opamp"
      }
      template_name = "./alert_nrql_templates/generic_metric_count.tftpl"
    },
  ]
}

