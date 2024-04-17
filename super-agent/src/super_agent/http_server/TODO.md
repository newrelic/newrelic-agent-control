* Health check type for Sub Agents is not being read from events. Maybe we should
  have `SubAgentAttributes` struct (
  see https://github.com/newrelic/newrelic-super-agent/pull/555#pullrequestreview-1991553259)
  and pass it along the event.
* Status for health component is not being handled in the Super Agent nor Sub Agents
  so it's not being handled either here.
