#!/bin/bash

TICK=0

while true
do
  echo "tick ${TICK}"
  sleep 1
  TICK=$((TICK+1))
done