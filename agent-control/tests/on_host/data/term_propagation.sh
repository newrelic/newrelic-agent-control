#!/bin/bash

# Usage: term_propagation.sh <marker-file>
# The root process ignores SIGTERM and never forwards it. A background child writes the
# marker file when it receives SIGTERM, so the marker only appears if the signal was
# delivered to the child directly.
marker="$1"

(
    trap 'echo terminated > "$marker"; exit 0' TERM
    while true; do sleep 0.1; done
) &

trap '' TERM
while true; do sleep 0.1; done
