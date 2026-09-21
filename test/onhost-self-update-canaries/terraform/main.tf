terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.48, < 6.0"
    }
    cloudinit = {
      source = "hashicorp/cloudinit"
    }
  }

  backend "s3" {
    bucket         = "agent-control-terraform-states"
    dynamodb_table = "agent-control-terraform-states"
    key            = "onhost-self-update-canaries.tfstate"
    region         = "us-east-2"
  }
}

provider "aws" {
  region = "us-east-2"
}

variable "ec2_prefix" {
  description = "Prefix for the canary instance names"
  type        = string
  default     = "self-update-canary"
}

variable "nr_region" {
  description = "New Relic region the install script and the Agent Control identity authenticate against"
  type        = string
  default     = "US"

  validation {
    condition     = can(regex("^(US|EU|Staging)$", var.nr_region))
    error_message = "Unsupported region"
  }
}

variable "subnet_id" {
  type    = string
  default = "subnet-00aa02e6d991b478e"
}

variable "security_group_id" {
  type    = string
  default = "sg-04ae18f8c34a11d38"
}

variable "key_name" {
  description = "EC2 key pair for manual VPN access; nothing in this module connects to the instances itself"
  type        = string
  default     = "caos-dev-arm"
}

variable "instance_type" {
  type    = string
  default = "t3a.small"
}

variable "license_key" {
  description = "New Relic license key passed to the CLI installer"
  type        = string
  sensitive   = true
}

variable "api_key" {
  description = "New Relic user API key passed to the CLI installer"
  type        = string
  sensitive   = true
}

variable "account_id" {
  description = "New Relic account ID the canaries report into"
  type        = string
}

variable "linux_fleet_id" {
  description = "Fleet Control fleet entity GUID the linux canary joins"
  type        = string
}

variable "windows_fleet_id" {
  description = "Fleet Control fleet entity GUID the windows canary joins"
  type        = string
}

variable "system_identity_client_id" {
  description = "Client ID of the pre-provisioned system identity both canaries authenticate as"
  type        = string
  sensitive   = true
}

variable "system_identity_private_key" {
  description = "Private key matching system_identity_client_id; written to disk at boot, never checked in"
  type        = string
  sensitive   = true
}

data "aws_ami" "ubuntu" {
  most_recent = true
  owners      = ["099720109477"] # Canonical

  filter {
    name   = "name"
    values = ["ubuntu/images/hvm-ssd-gp3/ubuntu-*-26.04-amd64-server-*"]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
}

data "aws_ami" "windows" {
  most_recent = true
  owners      = ["amazon"]

  filter {
    name   = "name"
    values = ["Windows_Server-2025-English-Full-Base-*"]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
}

locals {
  linux_private_key_path = "/etc/newrelic-agent-control/priv.key"
}

data "cloudinit_config" "linux" {
  gzip          = false
  base64_encode = true

  part {
    content_type = "text/cloud-config"
    content = yamlencode({
      write_files = [
        {
          path        = local.linux_private_key_path
          permissions = "0600"
          content     = var.system_identity_private_key
        }
      ]
      runcmd = [
        "curl -Ls https://download.newrelic.com/install/newrelic-cli/scripts/install.sh | bash",
        join(" ", [
          "NEW_RELIC_CLI_SKIP_CORE=1",
          "NEW_RELIC_LICENSE_KEY=${var.license_key}",
          "NEW_RELIC_API_KEY=${var.api_key}",
          "NEW_RELIC_ACCOUNT_ID=${var.account_id}",
          "NEW_RELIC_AUTH_PROVISIONED_CLIENT_ID=${var.system_identity_client_id}",
          "NEW_RELIC_AUTH_PRIVATE_KEY_PATH=${local.linux_private_key_path}",
          "NEW_RELIC_REGION=${var.nr_region}",
          "NR_CLI_FLEET_ID=${var.linux_fleet_id}",
          "NEW_RELIC_AGENT_CONTROL_FLEET_ENABLED=true",
          "/usr/local/bin/newrelic install -n agent-control",
        ])
      ]
    })
  }
}

resource "aws_instance" "linux" {
  ami                         = data.aws_ami.ubuntu.id
  instance_type               = var.instance_type
  key_name                    = var.key_name
  subnet_id                   = var.subnet_id
  vpc_security_group_ids      = [var.security_group_id]
  associate_public_ip_address = false
  user_data_base64            = sensitive(data.cloudinit_config.linux.rendered)
  user_data_replace_on_change = true

  tags = {
    Name = "${var.ec2_prefix}-linux"
  }
}

resource "aws_instance" "windows" {
  ami                         = data.aws_ami.windows.id
  instance_type               = var.instance_type
  key_name                    = var.key_name
  subnet_id                   = var.subnet_id
  vpc_security_group_ids      = [var.security_group_id]
  associate_public_ip_address = false
  user_data_replace_on_change = true

  user_data = sensitive(<<-EOF
    <powershell>
    [Net.ServicePointManager]::SecurityProtocol = 'tls12, tls'

    New-Item -ItemType Directory -Force -Path "C:\ProgramData\newrelic-agent-control" | Out-Null
    Set-Content -Path "C:\ProgramData\newrelic-agent-control\priv.key" -Value '${var.system_identity_private_key}'

    $WebClient = New-Object System.Net.WebClient
    $WebClient.DownloadFile("https://download.newrelic.com/install/newrelic-cli/scripts/install.ps1", "$env:TEMP\install.ps1")
    & PowerShell.exe -ExecutionPolicy Bypass -File "$env:TEMP\install.ps1"

    $env:NEW_RELIC_CLI_SKIP_CORE='1'
    $env:NEW_RELIC_LICENSE_KEY='${var.license_key}'
    $env:NEW_RELIC_API_KEY='${var.api_key}'
    $env:NEW_RELIC_ACCOUNT_ID='${var.account_id}'
    $env:NEW_RELIC_AUTH_PROVISIONED_CLIENT_ID='${var.system_identity_client_id}'
    $env:NEW_RELIC_AUTH_PRIVATE_KEY_PATH='C:\ProgramData\newrelic-agent-control\priv.key'
    $env:NEW_RELIC_REGION='${var.nr_region}'
    $env:NR_CLI_FLEET_ID='${var.windows_fleet_id}'
    $env:NEW_RELIC_AGENT_CONTROL_FLEET_ENABLED='true'
    & "C:\Program Files\New Relic\New Relic CLI\newrelic.exe" install -n agent-control
    </powershell>
  EOF
  )

  tags = {
    Name = "${var.ec2_prefix}-windows"
  }
}

output "linux_instance_id" {
  value = aws_instance.linux.id
}

output "windows_instance_id" {
  value = aws_instance.windows.id
}

output "linux_private_ip" {
  description = "Reachable only via the VPN connected to this VPC, same as the other canaries"
  value       = aws_instance.linux.private_ip
}

output "windows_private_ip" {
  description = "Reachable only via the VPN connected to this VPC. RDP as Administrator; get the password via `aws ec2 get-password-data --instance-id <windows_instance_id> --priv-launch-key ~/.ssh/caos-dev-arm.cer`."
  value       = aws_instance.windows.private_ip
}
