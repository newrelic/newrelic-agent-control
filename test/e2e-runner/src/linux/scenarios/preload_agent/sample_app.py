# Minimal application that reports whether the New Relic Python agent was loaded in its process.
import os
import sys

print(f"PYTHONPATH: {os.environ.get('PYTHONPATH', '')}")
print(f"NEW_RELIC_APP_NAME: {os.environ.get('NEW_RELIC_APP_NAME', '')}")

# The agent is loaded at interpreter startup, so it is already in the imported modules.
print(f"agent-attached: {'newrelic' in sys.modules}")
