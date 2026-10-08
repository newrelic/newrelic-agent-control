# Test OHI agent

A composite action that lets an agent's own CI prove that its **agent type** and **package** work with a real Agent Control (AC), asserted through NRQL.

It runs on a Linux or Windows runner and, in one execution:

1. Installs the latest New Relic CLI and uses it to install the latest released AC (local config only, no Fleet Control, no system identity) and the infrastructure agent.
2. Starts a local HTTPS OCI registry and publishes your package, your agent type and a mirror of the latest infrastructure agent package to it.
3. Configures AC to run the infrastructure agent plus your agent, with signature verification off, pulling everything from that registry.
4. Polls your NRQL until it returns a meaningful value, or fails at the timeout. On exit it dumps the AC logs and the `/status` output.

> [!IMPORTANT]
> A green run proves **only what your NRQL proves**. There is no negative control and no AC-side check. Write a query that cannot match data from other hosts or other runs, and that returns nothing (or zero) when your agent is not working.

Only on-host agents hosted by the infrastructure agent (OHIs that use `shared_filesystem`) are supported for now.

## Windows

Use a `windows-latest` runner and give the action the Windows files: an agent type with `operating_system: windows`, and a `.zip` package. The OHI binary is `<name>.exe` and the agent type refers to it with `\\` (for example `${nr-sub:packages.nri-vsphere.dir}\\nri-vsphere.exe`), as the embedded Windows agent types do. The package is published as `windows/amd64`, the only Windows build Agent Control ships.

On a Windows runner, `openssl` must be on the `PATH` (the one shipped with Git for Windows is enough), and the fake data source, if any, has to run natively: Linux containers are not available there.

## The run id and `{{test_id}}`

Every execution gets an id of the form `H.M.(seconds*1000+millis)` (for example `14.35.27123`). It is used as:

- the `version` of your agent type (the action rewrites the top-level `version:` line of the file it pushes, your file is not modified),
- the package tag and the `version` variable passed to your agent type,
- the `test.id` custom attribute of the infrastructure agent, and the value that replaces `{{test_id}}` in your NRQL.

Query on it to only match data of this run. Data reported through the infrastructure agent carries it as `test.id`:

```sql
SELECT * FROM VSphereHostSample WHERE `test.id` = '{{test_id}}' LIMIT 1
```

## Example: nri-vsphere

This workflow builds the package from the pull request's code, starts a fake vCenter (`vcsim`) and runs the action. Adapt the build step and file paths to your repository.

```yaml
name: Agent Control e2e
on: pull_request

jobs:
  agent-control-e2e:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-go@v5
        with:
          go-version-file: go.mod

      - name: Build the package
        run: |
          mkdir -p package
          go build -o package/nri-vsphere ./cmd/nri-vsphere
          cp vsphere-performance.metrics package/
          tar czf nri-vsphere.tar.gz -C package .

      - name: Start the fake vCenter
        run: docker run -d --name vcsim -p 8989:8989 vmware/vcsim:latest

      - uses: newrelic/agent-control/.github/actions/test-ohi-agent@main
        with:
          agent-type-file: .agent-control/agent-type.yaml
          package-file: nri-vsphere.tar.gz
          config-file: .agent-control/config.yaml
          nrql-assertion: "SELECT * FROM VSphereHostSample WHERE `test.id` = '{{test_id}}' LIMIT 1"
          nr-license-key: ${{ secrets.E2E_LICENSE_KEY }}
          nr-api-key: ${{ secrets.E2E_API_KEY }}
          nr-account-id: ${{ secrets.E2E_ACCOUNT_ID }}
```

`config.yaml` points the integration at the simulator:

```yaml
config:
  integrations:
    - name: nri-vsphere
      env:
        URL: https://127.0.0.1:8989/sdk
        USER: user
        PASS: pass
        ENABLE_VSPHERE_EVENTS: "true"
      interval: 60s
```
