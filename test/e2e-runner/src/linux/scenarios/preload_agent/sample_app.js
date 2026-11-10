// Minimal application that reports whether the New Relic Node.js agent was loaded in its process.
console.log(`NODE_OPTIONS: ${process.env.NODE_OPTIONS ?? ''}`);
console.log(`NEW_RELIC_APP_NAME: ${process.env.NEW_RELIC_APP_NAME ?? ''}`);

// The agent is preloaded with `--require`, so its modules are already in the require cache.
const agentLoaded = Object.keys(require.cache).some((module) =>
  module.includes('/node_modules/newrelic/'),
);
console.log(`agent-attached: ${agentLoaded}`);
