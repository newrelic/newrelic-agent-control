# Logs

## Log messages

Log messages **must begin with a capital letter and should not end with a period**.

Examples:

```rust
// 👍 Good:
debug!("Creating agent's communication channels");

// 👎 Bad:
// It must start with capital letter
debug!("creating agent's communication channels");
// It should not end with a period
debug!("Creating agent's communication channels.");
```

Log messages should generally be static, with fields used for dynamic content. However, the error message should be
included in the log message, even if it is static. The fields used for dynamic content should be `snake_case` and consistent.

Additionally, most dynamic fields (representing the log context) will be included automatically, as the code is traced
and logs are within the span context. Refer to the [tracing crate](https://docs.rs/tracing/latest/tracing/#core-concepts)
documentation and [this PR](https://github.com/newrelic/newrelic-agent-control/pull/1143)
for more details.

```rust
// 👍 Good:
info!(hash=&config.hash.get(), "Applying remote configuration");
warn!(hash=&config.hash.get(), "Remote configuration cannot be applied: {err}");

// 👎 Bad:
//`agent_id` is already added in the parent span, the error message in `err` should be part of the log message, fields should be snake_case and consistent.
info!(%agent_id, hash=&config.hash.get(), "Applying remote configuration");
warn!(%agent_id, configHash=&config.hash.get(), %err, "Remote configuration cannot be applied");
```

## Log level

Deciding which log level to use for each log message can be hard at times. We created this table to aid with the decision.

<table>
  <tr style="background-color:#f2f2f2;color:black;">
    <th>Log Type</th>
    <th>Situation</th>
    <th>General Examples</th>
    <th>AC Examples</th>
  </tr>
  <tr style="background-color:#ffcccc;color:black;">
    <td rowspan="2">Error</td>
    <td>Threatens the correct operation of AC</td>
    <td>
      <ul>
        <li>Invalid behaviours</li>
        <li>Potential application stop</li>
        <li>Potential data loss</li>
      </ul>
    </td>
    <td>
      <ul>
        <li>HTTP status server dies</li>
        <li>Channel is already closed and cannot communicate health (if this should never happen and should be considered a bug)</li>
      </ul>
    </td>
  </tr>
  <tr style="background-color:#ffcccc;color:black;">
    <td>Security issues</td>
    <td>
      <ul>
        <li>Invalid signature</li>
        <li>Three invalid authentications in a row</li>
      </ul>
    </td>
    <td>
      <ul>
        <li>Receiving a config incorrectly signed (could be an expired key or an attack)</li>
      </ul>
    </td>
  </tr>
  <tr style="background-color:#ffe5cc;color:black;">
    <td rowspan="2">Warn</td>
    <td>Impact AC behaviour without breaking the application</td>
    <td>
      <ul>
        <li>Subagent issues</li>
        <li>Some file system issues</li>
        <li>Some network issues</li>
      </ul>
    </td>
    <td>
      <ul>
        <li>Health cannot be checked (e.g., K8s API is not available or configured, sub-agent endpoint is not reachable)</li>
        <li>Channel is already closed and cannot communicate health (if this can be expected)</li>
      </ul>
    </td>
  </tr>
  <tr style="background-color:#ffe5cc;color:black;">
    <td>Issues that could be a problem in the future</td>
    <td>
      <ul>
        <li>Retries</li>
        <li>Temporal backup problems</li>
      </ul>
    </td>
    <td>
      <ul>
        <li>Supervisor restart retries</li>
      </ul>
    </td>
  </tr>
  <tr style="background-color:#ccffcc;color:black;">
    <td>Info</td>
    <td>General information for developers and users</td>
    <td>
      <ul>
        <li>Start some computation</li>
        <li>End some computation</li>
        <li>Send request</li>
        <li>Reading file</li>
      </ul>
    </td>
    <td>
      <ul>
        <li>Start agent control</li>
        <li>Start status server</li>
        <li>Start version checker</li>
        <li>Reading config file</li>
        <li>Getting new remote config</li>
      </ul>
    </td>
  </tr>
  <tr style="background-color:#cce5ff;color:black;">
    <td>Debug</td>
    <td>General information plus some internal details</td>
    <td>
      <ul>
        <li>Start some computation for “x”</li>
        <li>Got “y” from computation</li>
      </ul>
    </td>
    <td>
      <ul>
        <li>Start agent control on “x” version</li>
        <li>Reading config file from "path"</li>
        <li>Sending “x” event</li>
        <li>Reading “y” event</li>
        <li>Send “z” request</li>
      </ul>
    </td>
  </tr>
  <tr style="background-color:#f2f2f2;color:black;">
    <td>Trace</td>
    <td>Very detailed information about every step performed by AC to troubleshoot complex scenarios</td>
    <td>
      <ul>
        <li>OS, architecture, versions</li>
        <li>Data transformations</li>
        <li>Send request (with body, requests, URL, etc.)</li>
      </ul>
    </td>
    <td>
      <ul>
        <li>Detected environment (onhost, Kubernetes, etc.)</li>
        <li>Send request “r” to endpoint “e” with body “b” at time “t”</li>
      </ul>
    </td>
  </tr>
</table>
