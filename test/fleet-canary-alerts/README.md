# Agent Control fleet canary alerting

Fleet-level health alerting for the Agent Control canaries, driven by Fleet Control's
`AgentHeartbeat` events.

## Why this is separate from `onhost-canaries` and `k8s-canaries`

The on-host and k8s canary configs alert on **per-instance telemetry** (ProcessSample,
K8sContainerSample, self-instrumentation Logs) that lands in the canary **telemetry** account.

Agent **health**, on the other hand, is reported by Fleet Control **server-side** as `AgentHeartbeat`
events, which land in a separate **fleet-data** account (`12213068` staging / `6425865` production).
Each heartbeat carries a structured `agentHealthStatus` boolean keyed by `agentType` and `fleetGuid`,
so one fleet-wide query covers every managed agent across every canary fleet — on-host and k8s — at
once. Because that data is account-wide (not per host/cluster), this alert is defined **once** in its
own root config rather than duplicated in the on-host and k8s configs. It replaces the per-instance,
log-based "supervisor unhealthy" conditions that used to live in those configs.

## Where the alert lives (cross-account)

The alert resources (policy, condition, Slack/email destinations, workflow) are created in the canary
**telemetry** account — the *same* account and API key as the other canary alerts, where we have
write access. The NRQL condition then uses `data_account_id` to evaluate its query **against the
fleet-data account**, cross-account. This means we only need **read** access to the fleet-data account
(which is available), not write — avoiding the alert/notification create permissions we don't have
there.

| Env | Alert created in (`account_id`, write) | Queries (`data_account_id`, read) | Region |
|-----|----------------------------------------|-----------------------------------|--------|
| staging    | `12213067` (`NEW_RELIC_ACCOUNT_ID`)      | `12213068` | `Staging` |
| production | `NEW_RELIC_PROD_ACCOUNT_ID`              | `6425865`  | `US`      |

## The alert

Fires per `agentType`/`fleetGuid` that reports unhealthy continuously for 15 minutes — persistent,
not a transient blip:

```sql
SELECT count(*) FROM AgentHeartbeat WHERE agentHealthStatus IS FALSE FACET agentType, fleetGuid
```

Advantages over the previous log-based approach: it's a stable, structured, server-side signal
(no `Debug`-string parsing), it's per agent-type, it covers all managed agents, and it needs neither
self-instrumentation nor any Agent Control code change.

## Deploying

Via the make targets (matches CI — reads `NEW_RELIC_ACCOUNT_ID`/`NEW_RELIC_API_KEY` etc. from the
environment; `CANARY_DIR=production` swaps in the `_PROD_` credentials):

```bash
CANARY_DIR=staging make test/fleet-canary-alerts/terraform-plan     # or -apply / -destroy
```

Or directly with terraform (staging defaults `account_id=12213067`, `data_account_id=12213068`, so
only the secrets are required):

```bash
cd terraform/staging   # or terraform/production
terraform init -reconfigure
terraform apply \
  -var "api_key=$NEW_RELIC_API_KEY" \
  -var "slack_webhook_url=$SLACK_WEBHOOK_URL" \
  -var "emails=$EMAILS"
```

> **Note:** `api_key` must have **write** on the telemetry account (`account_id`) and **read** on the
> fleet-data account (`data_account_id`). For production, `account_id` has no default — pass the
> production canary telemetry account (`NEW_RELIC_PROD_ACCOUNT_ID`).
