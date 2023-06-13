#!/bin/sh

TICK=0

>&1 echo "out ticker started"
>&2 echo "TICKER WITHOUT STDERR"

while [ $TICK -lt 10 ]; do
  >&1 echo "ok tick $TICK"
  sleep .1
  TICK=$((TICK+1))
done
