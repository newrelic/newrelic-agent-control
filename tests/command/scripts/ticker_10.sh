#!/bin/sh

TICK=0

while [ $TICK -lt 10 ]; do
  >&2 echo "err tick $TICK"
  >&1 echo "ok tick $TICK"
  sleep .1
  TICK=$((TICK+1))
done
