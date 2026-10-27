# Test OHI agent

A composite action that lets the CI of an OHI prove that its **agent type** and **package** work with a real Agent Control (AC), asserted through NRQL.

It runs on a Linux or Windows runner and, in one execution:

1. Installs the latest New Relic CLI and uses it to install the latest released AC (local config only, no Fleet Control, no system identity) and the infrastructure agent.
2. Starts a local HTTPS OCI registry and publishes your package, your agent type and a mirror of the infrastructure agent package to it. The infrastructure agent version is always the latest one resolved from Instrumentation Metadata, and it is not configurable.
3. Configures AC to run the infrastructure agent plus your agent, with signature verification off, pulling everything from that registry.
4. Polls your NRQL until it returns a meaningful value, or fails after 10 minutes. On exit it dumps the AC logs and the `/status` output.

> [!IMPORTANT]
> A green run proves **only what your NRQL proves**. There is no negative control and no AC-side check. Write a query that cannot match data from other hosts or other runs, and that returns nothing (or zero) when your agent is not working.

## The test id and `{{test_id}}`

Every execution gets a test id of the form `H.M.(seconds*1000+millis)` (for example `14.35.27123`). It is used as:

- the `version` of your agent type (the action rewrites the top-level `version:` line of the file it pushes, your file is not modified),
- the package tag and the `version` variable passed to your agent type,
- the `test.id` custom attribute of the infrastructure agent, and the value that replaces `{{test_id}}` in your NRQL.

Query on it to only match data of this run. Data reported through the infrastructure agent carries it as `test.id`:

```sql
SELECT * FROM VSphereHostSample WHERE `test.id` = '{{test_id}}' LIMIT 1
```
