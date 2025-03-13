locals {
  assembled_ec2_configs = var.ec2_prefix == "" ? var.ec2_configs : { for k, v in var.ec2_configs : format("%s%s%s", var.ec2_prefix, ":", k) => v }
}

module "ec2_instances" {
  source  = "registry.terraform.io/terraform-aws-modules/ec2-instance/aws"

  for_each               = local.assembled_ec2_configs

  name                   = each.key
  ami                    = each.value.ami
  instance_type          = each.value.instance_type
  key_name               = each.value.key_name
  subnet_id              = each.value.subnet
  vpc_security_group_ids = each.value.security_groups
}

resource "null_resource" "wait_linux" {
  for_each = local.assembled_ec2_configs

  provisioner "remote-exec" {
    connection {
      type        = "ssh"
      user        = each.value.username
      host        = module.ec2_instances[each.key].private_ip
      private_key = file(var.ssh_key_file)
    }

    inline = [
      "echo 'connected'"
    ]
  }
}

resource "local_file" "AnsibleInventory" {
  depends_on = [null_resource.wait_linux]

  content = templatefile("../ansible/inventory-template.tmpl",
    {
      host-ids          = [for k, p in module.ec2_instances : k],
      host-user         = [for k, p in module.ec2_instances : local.assembled_ec2_configs[k].username],
      host-private-ip   = [for k, p in module.ec2_instances : p.private_ip],
      host-unique-id = [for k, p in module.ec2_instances : local.assembled_ec2_configs[k].metrics_attributes_id],
    }
  )
  filename = var.inventory_output
}

resource "null_resource" "ansible" {
  depends_on = [local_file.AnsibleInventory]

  triggers = {
    always_run = "${timestamp()}"
  }

  provisioner "local-exec" {
    command = "ANSIBLE_HOST_KEY_CHECKING=False ansible-playbook -i ${var.inventory_output} -e nr_license_key=${var.license_key} -e repo_endpoint=${var.repository_endpoint} --private-key ${var.ssh_key_file} ${var.ansible_playbook}"
  }
}