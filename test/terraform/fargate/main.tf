provider "aws" {
  region = "us-east-2"
}

#########################################
# State Backend
#########################################
terraform {
  backend "s3" {
    bucket         = "agent-control-terraform-states"
    dynamodb_table = "agent-control-terraform-states"
    key            = "fargate_cluster/terraform-states-backend.tfstate"
    region         = "us-east-2"
  }
}


module "agent_control_infra" {
  source                  = "github.com/newrelic/fargate-runner-action//terraform/modules/infra-ecs-fargate?ref=main"
  region                  = var.region
  vpc_id                  = var.vpc_id
  vpc_subnet_id           = var.vpc_subnet
  account_id              = var.accountId
  s3_terraform_bucket_arn = var.s3_bucket
  cluster_name            = var.cluster_name
  cloudwatch_log_group    = var.task_logs_group
  task_container_image    = var.task_container_image
  task_container_name     = var.task_container_name
  task_name_prefix        = var.task_name_prefix
  task_secrets = [
    # All canaries, e2e and packaging tests points to the same account.
    {
      "name" : "NR_LICENSE_KEY",
      "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_license}"
    },
    {
      "name" : "NEW_RELIC_ACCOUNT_ID",
      "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_account_id}"
    },
    {
      "name" : "NEW_RELIC_API_KEY",
      "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_api_key}"
    },
    {
      "name" : "NR_ORGANIZATION_ID",
      "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_organization_id}"
    },
    ####
    {
      "name" : "SSH_KEY",
      "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_ssh}"
    },
    {
      "name" : "CROWDSTRIKE_CLIENT_ID",
      "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_crowdstrike_client_id}"
    },
    {
      "name" : "CROWDSTRIKE_CLIENT_SECRET",
      "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_crowdstrike_client_secret}"
    },
    {
      "name" : "CROWDSTRIKE_CUSTOMER_ID",
      "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_crowdstrike_customer_id}"
    },
    {
      "name" : "CROWDSTRIKE_ANSIBLE_ROLE_KEY",
      "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_crowdstrike_ansible_role_key}"
    }
  ]
  task_custom_policies = [
    jsonencode(
      {
        "Version" : "2012-10-17",
        "Statement" : [

          {
            "Effect" : "Allow",
            "Action" : [
              "secretsmanager:GetSecretValue"
            ],
            "Resource" : [
              "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_ssh}",
              "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_license}",
              "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_account_id}",
              "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_api_key}",
              "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_organization_id}",
              "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_crowdstrike_client_id}",
              "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_crowdstrike_client_secret}",
              "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_crowdstrike_customer_id}",
              "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_crowdstrike_ansible_role_key}"
            ]
          }
        ]
      }
    )
  ]

  efs_volume_mount_point              = var.efs_volume_mount_point
  efs_volume_name                     = var.efs_volume_name
  additional_efs_security_group_rules = var.additional_efs_security_group_rules
  canaries_security_group             = var.canaries_security_group
  oidc_repository                     = var.oidc_repository
  oidc_role_name                      = var.oidc_role_name
  task_runtime_custom_policies        = var.task_runtime_custom_policies
  tags                                = var.tags
}
