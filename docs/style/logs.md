# Logs

Deciding which log level to use for each log message can be hard at times. We created this table to aid with the decision.
 
<table>
  <tr style="background-color:#f2f2f2;color:black;">
    <th>Log Type</th>
    <th>Situation</th>
    <th>General Examples</th>
    <th>AC Examples</th>
  </tr>
  <tr style="background-color:#ffcccc;color:black;">
    <td>Error</td>
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
    <td>Error</td>
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
    <td>Warn</td>
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
    <td>Warn</td>
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

