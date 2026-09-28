## Context

Dedicated OIDC-assumable IAM role for onhost canary workflows that run terraform directly on a
GitHub-hosted runner.

## Assume the dev role locally

Test terraform locally with the same permissions GitHub Actions has, instead of your full-admin SSO
identity, by adding a profile to `~/.aws/config`:

```ini
[profile onhost-canaries-dev]
role_arn = arn:aws:iam::<account>:role/dev-onhost-canaries
source_profile = default
```

`source_profile` needs `sts:AssumeRole` permission (your SSO profile has this). Then:

```bash
AWS_PROFILE=onhost-canaries-dev terraform ...
```
