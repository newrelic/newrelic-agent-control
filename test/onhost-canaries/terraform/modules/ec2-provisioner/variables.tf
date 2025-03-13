variable "license_key" {
  description = "NR License Key"
  type = string
}

variable "inventory_output" {
  description = "Path to write the inventory file"
  type = string
}

variable "ec2_prefix" {
  description = "Prefix for EC2 instances"
  type = string
}

variable "ec2_configs" {
    description = "EC2 configurations"
    type = map(any)
}

variable "ssh_key_file" {
  description = "SSH key file to use for remote execution"
  type = string
}

variable "repository_endpoint" {
  description = "Agent Control Repository Endpoint"
  type = string
}

variable "ansible_playbook" {
  description = "Ansible playbook file"
  type = string
}