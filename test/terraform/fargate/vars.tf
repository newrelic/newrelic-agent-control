#######################################
# Global vars
#######################################

variable "region" {
  default = "us-east-2"
}

variable "accountId" {
  default = "018789649883"
}

variable "vpc_id" {
  default = "vpc-0a3c00f5dc8645fe0"
}

variable "vpc_subnet" {
  default = "subnet-09b64de757828cdd4"
}

variable "cluster_name" {
  default = "caos_super_agent_releases"
}

#######################################
# Task definition
#######################################

variable "task_command" {
  default = [
    "test/automated-run"
  ]
}

variable "secret_name_ssh" {
  default = "caos/canaries/ssh_key-UBSKNA"
}

variable "secret_name_license" {
  default = "caos/canaries/license-f9eYwe"
}

variable "secret_name_license_canaries" {
  default = "caos/canaries/license_canaries-1DCE1L"
}

variable "secret_name_account" {
  default = "caos/canaries/account-kKFMGP"
}

variable "secret_name_api" {
  default = "caos/canaries/api-9q0NPb"
}

variable "secret_name_nr_api_key" {
  default = "caos/canaries/nr_api_key-xadBYJ"
}

variable "secret_name_docker_username" {
  default = "caos/canaries/docker-username-iksa0V"
}

variable "secret_name_docker_password" {
  default = "caos/canaries/docker-password-jAtw3v"
}

# CrowdStrike Falcon secrets

variable "secret_name_crowdstrike_client_id" {
  default = "caos/canaries/crowdstrike_falcon_client_id-N7nGXx"
}

variable "secret_name_crowdstrike_client_secret" {
  default = "caos/canaries/crowdstrike_falcon_client_secret-l9EIhi"
}

variable "secret_name_crowdstrike_customer_id" {
  default = "caos/canaries/crowdstrike_falcon_customer_id-f7n7rI"
}

####

variable "task_container_image" {
  default = "ghcr.io/newrelic/fargate-runner-action:latest"
}

variable "task_logs_group" {
  default = "/ecs/test-prerelease-super_agent-releases"
}

variable "task_container_name" {
  default = "test-super_agent-releases"
}

variable "task_name_prefix" {
  default = "super_agent-releases"
}

variable "task_logs_prefix" {
  default = "ecs-super_agent-releases"
}

variable "s3_bucket" {
  default = "arn:aws:s3:::automation-pipeline-terraform-state"
}

#######################################
# EFS volume
#######################################

variable "efs_volume_mount_point" {
  default = "/srv/runner/inventory"
}

variable "efs_volume_name" {
  default = "shared-super_agent-releases"
}

variable "canaries_security_group" {
  default = "sg-044ef7bc34691164a"
}

variable "additional_efs_security_group_rules" {
    default = [
    {
      type                     = "ingress"
      from_port                = 0
      to_port                  = 65535
      protocol                 = "tcp"
      cidr_blocks              = ["10.10.0.0/24"]
      description              = "Allow ingress traffic to EFS from trusted subnet"
    }
  ]
}

#######################################
# OIDC permissions
#######################################

variable "oidc_repository" {
  default = "repo:newrelic/newrelic-super-agent:*"
}

variable "oidc_role_name" {
  default = "caos-pipeline-oidc-super_agent-releases"
}
