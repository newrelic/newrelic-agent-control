#!/usr/bin/env bash
# Builds an isolated repo that simulates a branch whose author forgot the docs.
#   main    = parent of <commit>
#   feature = code of <commit> with docs/ left at the parent's version
# With <top-commit> the case is a branch stacked on another branch:
#   main    = parent of <commit>
#   parent  = code of <top-commit>^ with <commit>'s doc edits reverted, so docs/ is stale only for <commit>
#   feature = code of <top-commit> with docs/ as in `parent`, so docs/ is also stale for <top-commit>
# <top-commit> must descend from <commit>.
# Usage: build_fixture.sh <commit> <output-dir> [<top-commit>]   (run inside the project repo, or set DOCS_SYNC_REPO)
set -euo pipefail

commit=${1:?commit}
out=${2:?output dir}
top=${3:-}
repo=${DOCS_SYNC_REPO:-$(git rev-parse --show-toplevel)}
git_t() { git -c user.name=docs-sync-eval -c user.email=eval@example.invalid "$@"; }

# Replaces the working tree (except .git) with the files of <commit>, docs/ taken from <docs-commit>.
checkout_tree() {
  find . -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
  git -C "$repo" archive "$1" | tar -x
  rm -rf docs && git -C "$repo" archive "$2" docs | tar -x
}

rm -rf "$out" && mkdir -p "$out" && cd "$out"
git init -q -b main
git -C "$repo" archive "$commit^" | tar -x
git add -A && git_t commit -qm base

if [ -z "$top" ]; then
  git checkout -q -b feature
  checkout_tree "$commit" "$commit^"
  git add -A && git_t commit -qm feature
  echo "built $out: $(git diff main...HEAD --stat | tail -1); docs files in diff: $(git diff main...HEAD --name-only -- docs | wc -l | tr -d ' ')"
  exit 0
fi

git -C "$repo" merge-base --is-ancestor "$commit" "$top" || { echo "$top does not descend from $commit" >&2; exit 1; }
git checkout -q -b parent
checkout_tree "$top^" "$top^"
git -C "$repo" diff "$commit^" "$commit" -- docs | git apply -R
git add -A && git_t commit -qm parent

git checkout -q -b feature
find . -mindepth 1 -maxdepth 1 ! -name .git ! -name docs -exec rm -rf {} +
git -C "$repo" archive "$top" | tar -x --exclude='docs/*' --exclude=docs
git add -A && git_t commit -qm feature
echo "built $out: parent...feature $(git diff parent...HEAD --stat | tail -1); docs files in that diff: $(git diff parent...HEAD --name-only -- docs | wc -l | tr -d ' ')"
