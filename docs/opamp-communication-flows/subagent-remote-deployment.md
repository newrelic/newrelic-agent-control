# Sub-agent remote deployment

Agent Control installation will usually include local configuration for the sub-agents included in its local configuration.
For instance, if the Agent Control local configuration includes one sub-agent. Example:

```yaml
agents:
  nr-infra: newrelic/com.newrelic.infrastructure:0.1.0
```

It will include local configuration for the `nr-infra` agent.

However, a different set of agents can be deployed remotely by sending a remote configuration for Agent Control. When such
configuration is received, Agent Control will start the OpAMP communication for the new agents but it will not
start any agent. Since there is no local configuration, the agent will simply wait for remote configuration.

If the Agent Control described in the previous example receives the following remote configuration:

```yaml
agents:
  nr-prometheus: newrelic/com.newrelic.prometheus:0.1.0
```

* It will stop the `nr-infra` agent.
* It will open the OpAMP communication for the new agent `nr-prometheus`. Since Agent Control doesn't have any local configuration for `nr-prometheus` no agent will start and the agent will wait for a remote configuration.

If the server provides a remote configuration for `nr-prometheus` such as:

```yaml
chart_version: "*"
chart_values: {}
```

Agent Control will start the `nr-prometheus` agent with the remote configuration.

It is important to note the difference when the Agent Control has local configuration for an agent. If we remotely added `nr-infra` back,
it will start the corresponding agent with the known local configuration. In other words, a remote new remote config such as:

```yaml
agents:
  nr-infra: newrelic/com.newrelic.infrastructure:0.1.0
  nr-prometheus: newrelic/com.newrelic.prometheus:0.1.0
```

* Will keep the `nr-prometheus` agent running with the previously informed remote configuration. 
* Will start `nr-infra` agent with the local configuration.
