# Minimal application that reports whether the New Relic Ruby agent was attached to its process.
# The environment is printed first, since Bundler adds its own entry to RUBYOPT when loaded.
puts "RUBYOPT: #{ENV['RUBYOPT'].inspect}"
puts "NEW_RELIC_APP_NAME: #{ENV['NEW_RELIC_APP_NAME']}"
puts "NEW_RELIC_CONFIG_PATH: #{ENV['NEW_RELIC_CONFIG_PATH'].inspect}"

# The agent is introduced through Bundler, so the application requires its gems the usual way.
require 'bundler/setup'
Bundler.require

puts "agent-attached: #{defined?(::NewRelic::Agent) ? true : false}"
