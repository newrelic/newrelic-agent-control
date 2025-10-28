#!/bin/bash

# Trap SIGTERM (Signal 15) and set its action to ignore.
trap '' SIGTERM

echo "Script started. PID: $$"
echo "Waiting forever. Send SIGTERM (kill \$PID) to see it ignored."
echo "Send SIGKILL (kill -9 \$PID) to stop forcefully."

while true; do
    sleep 5
done
