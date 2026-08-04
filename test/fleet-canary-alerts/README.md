# Agent Control fleet canary alerting

Fleet-level health alerting for the Agent Control canaries, driven by Fleet Control's
`AgentHeartbeat` events.

## Why this is separate from `onhost-canaries` and `k8s-canaries`

The on-host and k8s canary configs alert on **per-instance telemetry** (ProcessSample,
K8sContainerSample, self-instrumentation Logs) that lands in the canary **telemetry** account.

Agent **health**, on the other hand, is reported by Fleet Control **server-side** as `AgentHeartbeat`
events, which land in a separate **fleet-data** account (`12213068` staging / `6425865` production).
Each heartbeat carries a structured `agentHealthStatus` boolean keyed by `agentType` and `fleetGuid`,
so one query — restricted to **our** canary fleets (see the filter note below) — covers every managed
agent across those fleets, on-host and k8s, at once. Because it's a single fleet-level query (not per
host/cluster), this alert is defined **once** in its own root config rather than duplicated in the
on-host and k8s configs.

| Env | Alert created in (`account_id`, write) | Queries (`data_account_id`, read) | Region |
|-----|----------------------------------------|-----------------------------------|--------|
| staging    | `12213067` (`NEW_RELIC_ACCOUNT_ID`)      | `12213068` | `Staging` |
| production | `NEW_RELIC_PROD_ACCOUNT_ID`              | `6425865`  | `US`      |

## The alert

Fires per `agentType`/`fleetGuid` that reports unhealthy continuously for 15 minutes:

```sql
SELECT filter(count(*), WHERE agentHealthStatus IS FALSE)
FROM AgentHeartbeat
WHERE fleetGuid IN (<our canary fleet GUIDs>)
FACET agentType, fleetGuid
```

> **The `fleetGuid IN (…)` filter is not optional.** The fleet-data account holds
> `AgentHeartbeat` for many fleets — in **production (`6425865`) it's the entire customer base** — so
> without this filter the alert would page us for *any customer's* unhealthy agents.
> The allow-list is the `fleet_guids` variable per environment, which must match the `FLEET_ID_*`
> / `WINDOWS_FLEET_ID_*` values in `component_onhost_canaries.yml` and `component_k8s_canaries.yml`
> (the canaries' actual fleets). `filter(count(*), …)` (rather than `WHERE agentHealthStatus IS FALSE`
> makes a recovered fleet emit `0` so its incident closes.

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
