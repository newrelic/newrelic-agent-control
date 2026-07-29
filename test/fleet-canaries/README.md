# Agent Control fleet canary alerting

Fleet-level health alerting for the Agent Control canaries, driven by Fleet Control's
`AgentHeartbeat` events.

## Why this is separate from `onhost-canaries` and `k8s-canaries`

The on-host and k8s canary configs alert on **per-instance telemetry** (ProcessSample,
K8sContainerSample, self-instrumentation Logs) that lands in the canary **telemetry** account.

Agent **health**, on the other hand, is reported by Fleet Control **server-side** as `AgentHeartbeat`
events, which land in a separate **fleet-data** account (`12213068` for staging). Each heartbeat
carries a structured `agentHealthStatus` boolean keyed by `agentType` and `fleetGuid`, so one
fleet-wide query covers every managed agent across every canary fleet — on-host and k8s — at once.

Because that data is account-wide (not per host/cluster) and lives in a different account than the
per-instance alerts, this alert is defined **once** in its own root config rather than duplicated in
the on-host and k8s configs (which would collide in the shared fleet account). It replaces the
per-instance, log-based "supervisor unhealthy" conditions that used to live in those configs.

## The alert

Fires per `agentType`/`fleetGuid` that reports unhealthy continuously for 15 minutes — persistent,
not a transient blip:

```sql
SELECT count(*) FROM AgentHeartbeat WHERE agentHealthStatus IS FALSE FACET agentType, fleetGuid
```

Advantages over the previous log-based approach: it's a stable, structured, server-side signal
(no `Debug`-string parsing), it's per agent-type, it covers all managed agents, and it needs neither
self-instrumentation nor any Agent Control code change.

## Environments

Staging and production are separate New Relic platforms, so each has its own directory with its own
account and provider region — exactly like the on-host and k8s canary configs:

| Dir | NR region | Fleet-data account |
|-----|-----------|--------------------|
| `terraform/staging`    | `Staging` | `12213068` |
| `terraform/production` | `US`      | `6425865`  |

## Deploying

```bash
cd terraform/staging   # or terraform/production
terraform init -reconfigure
terraform apply \
  -var "api_key=$NEW_RELIC_API_KEY" \
  -var "slack_webhook_url=$SLACK_WEBHOOK_URL" \
  -var "emails=$EMAILS"
```

> **Important:** `account_id` defaults to the **fleet-data** account that receives `AgentHeartbeat`
> (`12213068` staging / `6425865` production), which is **not** the canary telemetry account used by
> the other canary configs. The provided `api_key` must have write access to that account.
