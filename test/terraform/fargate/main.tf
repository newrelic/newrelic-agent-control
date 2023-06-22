provider "aws" {
  region = "us-east-2"
}

#########################################
# State Backend
#########################################
terraform {
  backend "s3" {
    bucket = "automation-pipeline-terraform-state"
    key    = "meta_agent_releases"
    region = "us-east-2"
  }
}


module "meta_agent_infra" {
    source = "github.com/newrelic/fargate-runner-action//terraform/modules/infra-ecs-fargate?ref=main"
    region = var.region
    vpc_id = var.vpc_id
    vpc_subnet_id = var.vpc_subnet
    account_id = var.accountId

    s3_terraform_bucket_arn = var.s3_bucket



    cluster_name           = var.cluster_name

    cloudwatch_log_group = var.task_logs_group

    task_container_image = var.task_container_image
    task_container_name = var.task_container_name
    task_name_prefix = var.task_name_prefix
    task_secrets = [
        {
          "name" : "SSH_KEY",
          "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_ssh}"
        },
        {
          "name" : "NR_LICENSE_KEY",
          "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_license}"
        },
        {
          "name" : "NR_LICENSE_KEY_CANARIES",
          "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_license_canaries}"
        },
        {
          "name" : "NEW_RELIC_ACCOUNT_ID",
          "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_account}"
        },
        {
          "name" : "NEW_RELIC_API_KEY",
          "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_api}"
        },
        {
          "name" : "NR_API_KEY",
          "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_nr_api_key}"
        },
        {
          "name" : "DOCKER_USERNAME",
          "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_docker_username}"
        },
        {
          "name" : "DOCKER_PASSWORD",
          "valueFrom" : "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_docker_password}"
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
                  "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_license_canaries}",
                  "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_account}",
                  "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_api}",
                  "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_nr_api_key}",
                  "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_docker_username}",
                  "arn:aws:secretsmanager:${var.region}:${var.accountId}:secret:${var.secret_name_docker_password}"
                ]
              }
            ]
          }
        )
      ]

    efs_volume_mount_point = var.efs_volume_mount_point
    efs_volume_name = var.efs_volume_name
    additional_efs_security_group_rules = var.additional_efs_security_group_rules
    canaries_security_group = var.canaries_security_group

    oidc_repository = var.oidc_repository
    oidc_role_name = var.oidc_role_name

}
