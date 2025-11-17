#!/bin/bash

set -e

## Freeing space since 14GB are not enough anymore
df -ih
df -h
echo "Deleting android, dotnet, haskell, CodeQL, Python, swift to free up space"
sudo rm -rf /usr/local/lib/android /usr/share/dotnet /usr/local/.ghcup /opt/hostedtoolcache/CodeQL /opt/hostedtoolcache/Python /usr/share/swift
df -ih
df -h
