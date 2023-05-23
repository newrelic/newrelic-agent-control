#!/bin/bash

TICK=0

# Loop from 0 to 9
for _ in {0..9}; do
  >&2 echo "err tick $TICK"
  >&1 echo "ok tick $TICK"
  sleep .1
  TICK=$((TICK+1))
done