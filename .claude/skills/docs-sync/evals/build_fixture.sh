#!/usr/bin/env bash
# Builds an isolated repo that simulates a branch whose author forgot the docs.
#   main    = parent of <commit>
#   feature = code of <commit> with docs/ left at the parent's version
# Usage: build_fixture.sh <commit> <output-dir>   (run inside the project repo, or set DOCS_SYNC_REPO)
set -euo pipefail

commit=${1:?commit}
out=${2:?output dir}
repo=${DOCS_SYNC_REPO:-$(git rev-parse --show-toplevel)}
git_t() { git -c user.name=docs-sync-eval -c user.email=eval@example.invalid "$@"; }

rm -rf "$out" && mkdir -p "$out" && cd "$out"
git init -q -b main
git -C "$repo" archive "$commit^" | tar -x
git add -A && git_t commit -qm base

git checkout -q -b feature
find . -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
git -C "$repo" archive "$commit" | tar -x
rm -rf docs && git -C "$repo" archive "$commit^" docs | tar -x
git add -A && git_t commit -qm feature

echo "built $out: $(git diff main...HEAD --stat | tail -1); docs files in diff: $(git diff main...HEAD --name-only -- docs | wc -l | tr -d ' ')"
