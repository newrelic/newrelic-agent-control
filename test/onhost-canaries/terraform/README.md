# Onhost Canaries Terraform - Split Architecture

This directory contains the Terraform infrastructure for onhost canaries, **split into two independent modules** for better failure isolation and faster reruns.

## 🎯 Why Split?

**Benefits:**
- ✅ **Clearer failures** - Know immediately if infra or alerts failed
- ✅ **Faster reruns** - Only rerun the failed module (infra OR alerts, not both)
- ✅ **Independent job control** - Retry staging alerts without re-provisioning staging EC2s
- ✅ **Parallel execution** - Infra and alerts can run simultaneously in some cases

## 📁 Directory Structure

```
terraform/
├── infra/                    # Infrastructure module (EC2, networking, inventory)
│   ├── main.tf              # EC2 instances, wait resources, ansible inventory
│   ├── backend.tf           # AWS provider, S3 backend
│   └── environments/
│       ├── staging/
│       │   ├── backend.conf       # key: ...onhost-infra-staging.tfstate
│       │   └── inputs.tfvars      # ec2_prefix, inventory_output
│       └── production/
│           ├── backend.conf       # key: ...onhost-infra-production.tfstate
│           └── inputs.tfvars
│
├── alerts/                   # Alerts module (New Relic alerts, policies, workflows)
│   ├── main.tf              # NR alert policies, conditions, Slack notifications
│   ├── backend.tf           # New Relic provider, S3 backend
│   ├── alert_nrql_templates/
│   └── environments/
│       ├── staging/
│       │   ├── backend.conf       # key: ...onhost-alerts-staging.tfstate
│       │   └── inputs.tfvars      # nr_region, infra_backend_config
│       └── production/
│           ├── backend.conf       # key: ...onhost-alerts-production.tfstate
│           └── inputs.tfvars
│
└── (legacy files)            # Old monolithic structure (DEPRECATED)
    ├── main.tf
    ├── backend.tf
    └── environments/
```

## 🔧 Makefile Targets

### New Split Targets (USE THESE)

**Infrastructure only:**
```bash
make test/onhost-canaries/infra-only-plan ENVIRONMENT=staging
make test/onhost-canaries/infra-only-apply ENVIRONMENT=staging
make test/onhost-canaries/infra-only-destroy ENVIRONMENT=staging
```

**Alerts only:**
```bash
make test/onhost-canaries/alerts-plan ENVIRONMENT=staging
make test/onhost-canaries/alerts-apply ENVIRONMENT=staging
make test/onhost-canaries/alerts-destroy ENVIRONMENT=staging
```

### Legacy Targets (DEPRECATED)

These still exist for backward compatibility but run the old monolithic terraform:
```bash
make test/onhost-canaries/infra-plan ENVIRONMENT=staging
make test/onhost-canaries/infra-apply ENVIRONMENT=staging
make test/onhost-canaries/infra-destroy ENVIRONMENT=staging
```

## 🔄 GitHub Actions Workflows

### New Split Workflows

**Plan (on every push):**
- `.github/workflows/push_pr_onhost_canaries_split_plan.yml`
- Runs `infra-only-plan` and `alerts-plan` in parallel
- Triggered by changes to `terraform/infra/**` or `terraform/alerts/**`

**Apply (on push to main):**
- `.github/workflows/push_pr_onhost_canaries_split_apply.yml`
- Runs `infra-only-apply` first, then `alerts-apply`
- Sequential execution (alerts need infra outputs)

### Legacy Workflow (DEPRECATED)

- `.github/workflows/push_pr_onhost_canaries_plan.yml`
- Uses old monolithic terraform

## 🔗 How Alerts Module Reads Infra Outputs

The alerts module uses `terraform_remote_state` to read outputs from the infra module:

```hcl
data "terraform_remote_state" "infra" {
  backend = "s3"
  config = {
    bucket = "agent-control-terraform-states"
    key    = var.infra_backend_config  # e.g., "foundations/...onhost-infra-staging.tfstate"
    region = "us-east-2"
  }
}

locals {
  ec2_prefix    = data.terraform_remote_state.infra.outputs.ec2_prefix
  ec2_instances = data.terraform_remote_state.infra.outputs.ec2_instances
}
```

**This means:**
- Infra must be applied before alerts can plan/apply
- Alerts automatically pick up changes from infra state
- No manual synchronization needed

## 📊 State Files

**Separate S3 state files:**
- `foundations/terraform-states-backend-onhost-infra-staging.tfstate`
- `foundations/terraform-states-backend-onhost-infra-production.tfstate`
- `foundations/terraform-states-backend-onhost-alerts-staging.tfstate`
- `foundations/terraform-states-backend-onhost-alerts-production.tfstate`

**Legacy state files (DO NOT DELETE YET):**
- `foundations/terraform-states-backend-onhost-staging.tfstate`
- `foundations/terraform-states-backend-onhost-production.tfstate`

## 🚀 Migration Path

1. **Phase 1: Initial split (DONE)**
   - New modules created: `infra/` and `alerts/`
   - New Makefile targets added
   - New GHA workflows created

2. **Phase 2: Testing (IN PROGRESS)**
   - Test split workflows in non-production environments
   - Verify remote state data source works correctly
   - Validate independent module failures

3. **Phase 3: Cutover (TODO)**
   - Switch default workflows to split version
   - Deprecate old monolithic terraform
   - Update documentation

4. **Phase 4: Cleanup (TODO)**
   - Remove legacy files after confirming stability
   - Archive old state files

## ⚠️ Important Notes

- **Dependency:** Alerts module depends on infra outputs via remote state
- **Backward compatibility:** Legacy targets still work with old structure
- **Ansible inventory:** Generated by infra module, used by deployment workflows
- **Concurrency:** Each module has independent state locks (no blocking between infra and alerts)

## 🧪 Testing Checklist

- [ ] Run `infra-only-plan` for staging
- [ ] Run `infra-only-apply` for staging
- [ ] Verify EC2s are created and inventory file exists
- [ ] Run `alerts-plan` for staging
- [ ] Verify alerts can read ec2_prefix and ec2_instances from infra remote state
- [ ] Run `alerts-apply` for staging
- [ ] Verify NR alert policies are created
- [ ] Introduce failure in alerts terraform, verify infra is not affected
- [ ] Rerun only alerts, verify faster execution
