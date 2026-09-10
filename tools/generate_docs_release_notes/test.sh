#!/bin/bash
# Runs the generator against the test data and
# diffs the output against the expected .mdx files.
#
# Exits 0 only if ALL cases match byte-for-byte.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GENERATOR="$SCRIPT_DIR/run.sh"
TESTDATA="$SCRIPT_DIR/data"

# Each case: version, changelog, expected output.
CHANGELOGS=(changelog_full.md changelog_deps_only.md)
VERSIONS=(1.99.0 1.99.0)
EXPECTED_FILES=(expected_full.mdx expected_deps_only.mdx)

overall_rc=0

for i in "${!CHANGELOGS[@]}"; do
  changelog="$TESTDATA/${CHANGELOGS[$i]}"
  version="${VERSIONS[$i]}"
  expected="$TESTDATA/${EXPECTED_FILES[$i]}"

  # Run the generator inside the temp dir so the .mdx lands there.
  tmpdir="$(mktemp -d)"
  cd "$tmpdir" || exit 1

  out_path=""
  if out_path="$(bash "$GENERATOR" "$changelog" "$version")"; then
    produced="$tmpdir/$out_path"
    if diff -u "$expected" "$produced" > "$tmpdir/diff.txt"; then
      echo "PASS: $version (${EXPECTED_FILES[$i]})"
    else
      echo "FAIL: $version (${EXPECTED_FILES[$i]}) — output differs from expected:"
      cat "$tmpdir/diff.txt"
      overall_rc=1
    fi
  else
    echo "FAIL: $version (${EXPECTED_FILES[$i]}) — generator exited non-zero"
    overall_rc=1
  fi

  rm -rf "$tmpdir"
done

if [ "$overall_rc" -eq 0 ]; then
  echo "All cases passed."
else
  echo "One or more cases FAILED."
fi

exit "$overall_rc"
