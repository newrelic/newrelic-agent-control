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

  api_key         = var.api_key
  account_id      = var.account_id
  slack_webhook_url = var.slack_webhook_url
  policies_prefix = "Agent Control canaries metric monitoring"

  region       = "Staging"
  instance_id  = "Agent_Control_Canaries_Staging-Cluster"

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
      threshold     = 14000000 # 14 MB, +25% of observed value https://staging.onenr.io/0dQeV0JdVwe
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
      name          = "Agent Control container"
      metric        = "*"
      sample        = "K8sContainerSample"
      threshold     = 0
      duration      = 600
      operator      = "below_or_equals"
      template_name = "./alert_nrql_templates/generic_metric_count.tftpl"
    },
  ]
}

