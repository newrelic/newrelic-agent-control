# Agent Control self-update canaries

Two long-lived on-host canaries (one Ubuntu, one Windows) that exist to validate Agent Control's
own self-update path. They're provisioned once via Terraform/cloud-init and, after that, are never
touched by an installer or SSH/WinRM session again - every subsequent version change reaches them
purely through a Fleet Control deployment, the same way a real fleet self-updates.

## Local usage

```bash
terraform init
terraform apply \
  -var license_key=... \
  -var api_key=... \
  -var account_id=... \
  -var linux_fleet_id=... \
  -var windows_fleet_id=... \
  -var system_identity_client_id=... \
  -var system_identity_private_key="$(cat priv.key)"
```

`terraform destroy` with the same flags tears the canaries down.

The Windows Administrator password is never set explicitly - it's EC2Launch's default random
password, encrypted with `key_name`. Retrieve it for manual RDP debugging with:

```bash
aws ec2 get-password-data --instance-id <windows_instance_id> --priv-launch-key ~/.ssh/caos-dev-arm.cer
```
