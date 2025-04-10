#######################################
# Global vars
#######################################

variable "region" {
  default = "us-east-2"
}

variable "accountId" {
  default = "288761741714"
}

variable "vpc_id" {
  default = "vpc-0f62d8a55c8d9ad61"
}

variable "vpc_subnet" {
  default = "subnet-00aa02e6d991b478e"
}

variable "cluster_name" {
  default = "agent_control"
}

#######################################
# Task definition
#######################################

variable "tags" {
  type = map
  default = {
    owning_team = "agent_control"
  }
}

# Account secrets
variable "secret_name_license" {
  default = "canaries/license_key-5wRtkT"
}

variable "secret_name_prod_license" {
  default = "canaries/prod_license_key-Ats42t"
}

variable "secret_name_account_id" {
  default = "canaries/account_id-a25041"
}

variable "secret_name_prod_account_id" {
  default = "canaries/prod_account_id-aTmqLo"
}

variable "secret_name_api_key" {
  default = "canaries/nr_api_key-zHyTVS"
}

variable "secret_name_prod_api_key" {
  default = "canaries/prod_nr_api_key-dbXHer"
}

variable "secret_name_organization_id" {
  default = "canaries/organization_id-a0w4NX"
}

variable "secret_name_prod_organization_id" {
  default = "canaries/prod_organization_id-k0Fchg"
}

variable "secret_name_system_identity_client_id" {
  default = "canaries/system_identity_client_id-qVzy88"
}

variable "secret_name_prod_system_identity_client_id" {
  default = "canaries/prod_system_identity_client_id-Kw2stx"
}

variable "secret_name_system_identity_private_key" {
  default = "canaries/system_identity_private_key-G5vwsz"
}

variable "secret_name_prod_system_identity_private_key" {
  default = "canaries/prod_system_identity_private_key-8SNBQY"
}

variable "secret_name_super-secret-agent-slack-webhook" {
  default = "super-secret-agent-slack-webhook-WsH2yK"
}

####

variable "secret_name_ssh" {
  default = "canaries/ssh_key-yhthNk"
}

variable "task_container_image" {
  default = "ghcr.io/newrelic/fargate-runner-action:latest"
}

variable "task_logs_group" {
  default = "/ecs/test-prerelease-agent_control"
}

variable "task_container_name" {
  default = "test-agent_control"
}

variable "task_name_prefix" {
  default = "agent_control"
}

variable "s3_bucket" {
  default = "arn:aws:s3:::agent-control-terraform-states"
}

#######################################
# EFS volume
#######################################

variable "efs_volume_mount_point" {
  default = "/srv/runner/inventory"
}

variable "efs_volume_name" {
  default = "shared-agent_control"
}

variable "canaries_security_group" {
  default = "sg-04ae18f8c34a11d38"
}

variable "additional_efs_security_group_rules" {
  default = [
    {
      type        = "ingress"
      from_port   = 0
      to_port     = 65535
      protocol    = "tcp"
      cidr_blocks = ["10.10.0.0/24"]
      description = "Allow ingress traffic to EFS from trusted subnet"
    }
  ]
}

#######################################
# OIDC permissions
#######################################

// These permissions are the ones that the Fargate task can assume and execute.
variable "task_runtime_custom_policies" {
  type = list(string)
  description = "Custom policies for task runtime as JSON strings."
  default = [
    "{ \"Statement\": [{ \"Effect\": \"Allow\", \"Action\": \"eks:*\", \"Resource\": \"*\" }] }",
    "{ \"Statement\": [{ \"Effect\": \"Allow\", \"Action\": \"ec2:*\", \"Resource\": \"*\" }] }",
    "{ \"Statement\": [{ \"Effect\": \"Allow\", \"Action\": \"dynamodb:*\", \"Resource\": \"*\" }]}",
    "{ \"Statement\": [{ \"Effect\": \"Allow\", \"Action\": [\"iam:GetGroup\", \"iam:GetGroupPolicy\", \"iam:GetPolicy\", \"iam:GetPolicyVersion\", \"iam:GetRole\", \"iam:GetRolePolicy\", \"iam:GetUser\", \"iam:GetUserPolicy\", \"iam:ListAttachedGroupPolicies\", \"iam:ListAttachedRolePolicies\", \"iam:ListAttachedUserPolicies\", \"iam:ListGroups\", \"iam:ListGroupPolicies\", \"iam:ListGroupsForUser\", \"iam:ListRolePolicies\", \"iam:ListRoles\", \"iam:ListUserPolicies\", \"iam:ListUsers\"], \"Resource\": \"*\"}]}"
  ]
}

variable "oidc_repository" {
  default = "repo:newrelic/newrelic-agent-control:*"
}

variable "oidc_role_name" {
  default = "caos-pipeline-oidc-agent_control"
}
