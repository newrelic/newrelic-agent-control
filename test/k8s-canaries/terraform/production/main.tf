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
module "alerts" {
  source = "../modules/nr_alerts"

  api_key         = var.api_key
  account_id      = var.account_id
  policies_prefix = "Agent Control canaries metric monitoring"
  conditions = [
    {
      name          = "CPU usage (cores)"
      metric        = "cpuUsedCores"
      sample        = "K8sContainerSample"
      threshold     = 1
      duration      = 3600
      operator      = "above"
      template_name = "./alert_nrql_templates/generic_metric_threshold.tftpl"
    },
    {
      name          = "Memory usage (bytes)"
      metric        = "memoryWorkingSetBytes"
      sample        = "K8sContainerSample"
      threshold     = 10000000 # 10 MB
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
  ]
  region       = "US"
  cluster_name = "Agent_Control_Canaries_Production"
}
