#!/bin/sh

TICK=0

>&1 echo "TICKER WITHOUT STDOUT"
>&2 echo "err ticker started"

while [ $TICK -lt 10 ]; do
  >&2 echo "err tick $TICK"
  sleep .1
  TICK=$((TICK+1))
done
