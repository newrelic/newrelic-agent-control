variable "ec2_prefix" {
  description = "Prefix for EC2 instances"
  type        = string
}

variable "inventory_output" {
  description = "Path to write the inventory file"
  type        = string
}

variable "windows_password" {
  description = "Windows AMI password for WinRM connection"
  type        = string
}

variable "pvt_key_path" {
  description = "Path to SSH private key"
  type        = string
  default     = "~/.ssh/caos-dev-arm.cer"
}

locals {
  ec2_instances = {
    "amd64:ubuntu22.04" = {
      ami             = "ami-0884d2865dbe9de4b"
      subnet          = "subnet-00aa02e6d991b478e"
      security_groups = ["sg-04ae18f8c34a11d38"]
      key_name        = "caos-dev-arm"
      instance_type   = "t3a.small"
      username        = "ubuntu"
      platform        = "linux"
      python          = "/usr/bin/python3"
    }
    "amd64:windows_2022" = {
      ami             = "ami-04382be054853bd1f"
      subnet          = "subnet-00aa02e6d991b478e"
      security_groups = ["sg-04ae18f8c34a11d38"]
      key_name        = "caos-dev-arm"
      instance_type   = "t3a.small"
      username        = "Administrator"
      platform        = "windows"
      python          = ""
    }
  }

  # Append ec2_prefix to ec2 instances name
  assembled_ec2 = {
    for k, v in local.ec2_instances :
    format("%s-%s", var.ec2_prefix, replace(k, "/[:._]/", "-")) => v
  }
}

# Create EC2 instances
module "ec2_instances" {
  source  = "registry.terraform.io/terraform-aws-modules/ec2-instance/aws"
  version = "3.4.0"

  for_each = local.assembled_ec2

  name                   = each.key
  ami                    = each.value.ami
  instance_type          = each.value.instance_type
  key_name               = each.value.key_name
  subnet_id              = each.value.subnet
  vpc_security_group_ids = each.value.security_groups
}

# Wait for Linux instances to be ready
resource "null_resource" "wait_linux" {
  for_each = {
    for key, val in local.assembled_ec2 :
    key => val if val.platform == "linux"
  }

  provisioner "remote-exec" {
    connection {
      type        = "ssh"
      user        = each.value.username
      host        = module.ec2_instances[each.key].private_ip
      private_key = file(var.pvt_key_path)
    }

    inline = [
      "echo 'connected'"
    ]
  }

  depends_on = [module.ec2_instances]
}

# Wait for Windows instances to be ready
resource "null_resource" "wait_windows" {
  for_each = {
    for key, val in local.assembled_ec2 :
    key => val if val.platform == "windows"
  }

  provisioner "remote-exec" {
    connection {
      type     = "winrm"
      user     = each.value.username
      host     = module.ec2_instances[each.key].private_ip
      password = var.windows_password
      insecure = true
      https    = true
    }

    inline = [
      "echo 'connected'"
    ]
  }

  depends_on = [module.ec2_instances]
}

# Generate Ansible inventory file
resource "local_file" "ansible_inventory" {
  depends_on = [null_resource.wait_linux, null_resource.wait_windows]

  content = templatefile("${path.module}/../../ansible/inventory-template.tmpl",
    {
      gateway-ids        = []
      gateway-user       = []
      gateway-private-ip = []
      agent-ids          = [for k, p in module.ec2_instances : k]
      agent-python       = [for k, p in module.ec2_instances : local.assembled_ec2[k].python]
      agent-user         = [for k, p in module.ec2_instances : local.assembled_ec2[k].username]
      agent-private-ip   = [for k, p in module.ec2_instances : p.private_ip]
      instance-id        = [for k, p in module.ec2_instances : p.id]
      platform           = [for k, p in module.ec2_instances : local.assembled_ec2[k].platform]
      windows_password   = var.windows_password
    }
  )
  filename = var.inventory_output
}

# Outputs
output "ec2_instances" {
  description = "EC2 instances configuration"
  value       = local.ec2_instances
}

output "ec2_prefix" {
  description = "Prefix for EC2 instances"
  value       = var.ec2_prefix
}

output "instance_ids" {
  description = "EC2 instance IDs"
  value = {
    for k, v in module.ec2_instances : k => v.id
  }
}

output "instance_private_ips" {
  description = "EC2 instance private IPs"
  value = {
    for k, v in module.ec2_instances : k => v.private_ip
  }
}

output "inventory_file" {
  description = "Path to generated Ansible inventory file"
  value       = local_file.ansible_inventory.filename
}

output "ansible_inventory_content" {
  description = "Ansible inventory file content (generated from Terraform state)"
  value       = local_file.ansible_inventory.content
  sensitive   = true
}
