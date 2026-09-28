###################################################################################
# State Backend
###################################################################################
terraform {
  backend "s3" {
    bucket         = "agent-control-terraform-states"
    dynamodb_table = "agent-control-terraform-states"
    key            = "onhost_canaries_iam/terraform-states-backend.tfstate"
    region         = "us-east-2"
  }
}

provider "aws" {
  region = "us-east-2"
}

###################################################################################
# OIDC role for onhost canary workflows that run terraform directly on a GitHub-hosted
# runner.
#
###################################################################################

data "aws_caller_identity" "current" {}
data "aws_partition" "current" {}

locals {
  state_bucket_arn = "arn:${data.aws_partition.current.partition}:s3:::agent-control-terraform-states"
}

data "aws_iam_policy_document" "assume_role" {
  statement {
    effect  = "Allow"
    actions = ["sts:AssumeRoleWithWebIdentity"]

    principals {
      type        = "Federated"
      identifiers = ["arn:${data.aws_partition.current.partition}:iam::${data.aws_caller_identity.current.account_id}:oidc-provider/token.actions.githubusercontent.com"]
    }

    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:aud"
      values   = ["sts.amazonaws.com"]
    }

    condition {
      test     = "StringLike"
      variable = "token.actions.githubusercontent.com:sub"
      values   = ["repo:newrelic/newrelic-agent-control:*"]
    }
  }
}

resource "aws_iam_role" "onhost_canaries" {
  name               = "onhost-canaries-oidc"
  assume_role_policy = data.aws_iam_policy_document.assume_role.json
}

data "aws_iam_policy_document" "onhost_canaries" {
  statement {
    sid       = "TerraformStateList"
    actions   = ["s3:ListBucket"]
    resources = [local.state_bucket_arn]
  }

  statement {
    sid       = "TerraformStateObject"
    actions   = ["s3:GetObject", "s3:PutObject", "s3:DeleteObject"]
    resources = ["${local.state_bucket_arn}/*"]
  }

  statement {
    sid       = "TerraformStateLock"
    actions   = ["dynamodb:GetItem", "dynamodb:PutItem", "dynamodb:DeleteItem"]
    resources = ["arn:${data.aws_partition.current.partition}:dynamodb:us-east-2:${data.aws_caller_identity.current.account_id}:table/agent-control-terraform-states"]
  }

  statement {
    sid = "CanaryInstances"
    actions = [
      "ec2:DescribeImages",
      "ec2:DescribeInstances",
      "ec2:DescribeInstanceAttribute",
      "ec2:DescribeInstanceCreditSpecifications",
      "ec2:DescribeInstanceTypes",
      "ec2:DescribeTags",
      "ec2:DescribeSecurityGroups",
      "ec2:DescribeSubnets",
      "ec2:DescribeKeyPairs",
      "ec2:DescribeVolumes",
      "ec2:DescribeNetworkInterfaces",
      "ec2:RunInstances",
      "ec2:TerminateInstances",
      "ec2:CreateTags",
      "ec2:DeleteTags",
    ]
    resources = ["*"]
  }
}

resource "aws_iam_policy" "onhost_canaries" {
  name   = "onhost-canaries"
  policy = data.aws_iam_policy_document.onhost_canaries.json
}

resource "aws_iam_role_policy_attachment" "onhost_canaries" {
  role       = aws_iam_role.onhost_canaries.name
  policy_arn = aws_iam_policy.onhost_canaries.arn
}

###################################################################################
# Dev role: same policy as the OIDC role above, but assumable by any principal in
# this AWS account (via `aws sts assume-role`) instead of by GitHub Actions. Lets
# you test the actual restricted policy locally instead of applying/debugging with
# your full-admin SSO identity - see README.md.
###################################################################################

data "aws_iam_policy_document" "assume_role_dev" {
  statement {
    effect  = "Allow"
    actions = ["sts:AssumeRole"]

    principals {
      type        = "AWS"
      identifiers = [data.aws_caller_identity.current.account_id]
    }
  }
}

resource "aws_iam_role" "dev_onhost_canaries" {
  name        = "dev-onhost-canaries"
  description = "Assumable locally by account principals for testing the onhost-canaries policy"

  assume_role_policy = data.aws_iam_policy_document.assume_role_dev.json
}

resource "aws_iam_role_policy_attachment" "dev_onhost_canaries" {
  role       = aws_iam_role.dev_onhost_canaries.name
  policy_arn = aws_iam_policy.onhost_canaries.arn
}

output "role_arn" {
  value = aws_iam_role.onhost_canaries.arn
}

output "dev_role_arn" {
  value = aws_iam_role.dev_onhost_canaries.arn
}
