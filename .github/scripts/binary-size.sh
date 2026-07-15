#!/usr/bin/env bash
# Binary size tooling for CI. Subcommands:
#   measure        <archives-dir> <output-file>  # extract sizes from packaged tar.gz/zip (nightly/prerelease)
#   measure-local  <output-file> <name=path>...  # stat already-built local binaries (PR check)
#   check          <current-file> <baseline-file> [threshold-percent, default 10]
set -euo pipefail

# Binaries packaged by .goreleaser.yml; keep in sync by hand with Cargo.toml's [[bin]] entries.
RELEASE_ARCHIVE_BIN_NAMES='newrelic-agent-control newrelic-agent-control.exe newrelic-agent-control-cli newrelic-agent-control-cli.exe'

measure_size() {
  # Writes "<key> <size-bytes>" per binary; key = version-stripped archive name + binary name.
  local src="$1" out="$2" base key tmp size
  : > "$out"
  local bin_names="$RELEASE_ARCHIVE_BIN_NAMES"

  # Drop the version (2nd underscore-separated field) from a goreleaser archive base name.
  strip_version() {
    awk -F_ '{ out=$1; for (i=3; i<=NF; i++) out=out"_"$i; print out }'
  }

  while IFS= read -r archive; do
    base=$(basename "$archive")
    case "$base" in
      *.tar.gz) key=$(printf '%s' "${base%.tar.gz}" | strip_version) ;;
      *.zip)    key=$(printf '%s' "${base%.zip}" | strip_version) ;;
      *) continue ;;
    esac

    tmp=$(mktemp -d)
    case "$base" in
      *.tar.gz) tar -xzf "$archive" -C "$tmp" ;;
      *.zip)    unzip -q "$archive" -d "$tmp" ;;
    esac

    for name in $bin_names; do
      while IFS= read -r b; do
        [ -n "$b" ] || continue
        size=$(wc -c < "$b" | tr -d '[:space:]')
        if [ "$size" -eq 0 ]; then
          echo "::error::Extracted binary '$b' (from $archive) is 0 bytes; refusing to record a corrupt size." >&2
          exit 1
        fi
        echo "${key}::${name} ${size}" >> "$out"
      done < <(find "$tmp" -type f -name "$name")
    done

    rm -rf "$tmp"
  done < <(find "$src" -type f \( -name '*.tar.gz' -o -name '*.zip' \))

  sort -o "$out" "$out"
  cat "$out"
}

measure_local_size() {
  # Measures already-built local binaries; each arg is "<name>=<path>".
  local out="$1" pair name path size
  shift
  : > "$out"
  for pair in "$@"; do
    name="${pair%%=*}"
    path="${pair#*=}"
    size=$(stat -c%s "$path")
    if [ "$size" -eq 0 ]; then
      echo "::error::Binary '$path' is 0 bytes; refusing to record a corrupt size." >&2
      exit 1
    fi
    echo "${name} ${size}" >> "$out"
  done
  cat "$out"
}

check_size() {
  # Fails when a binary grows more than THRESHOLD% over baseline; sizes files are "<name> <size>" lines.
  local current_file="$1" baseline_file="$2" threshold="${3:-10}"
  local summary="${GITHUB_STEP_SUMMARY:-/dev/stdout}"
  local status=0 name current baseline delta pretty exceeds icon delta_h

  # Renders a plain or "archive::binary" key as a friendly `binary` (platform) label.
  label() {
    case "$1" in
      *::*)
        local arch="${1%%::*}" bin="${1##*::}"
        printf '`%s` (%s)' "$bin" "${arch#*_}"
        ;;
      *) printf '`%s`' "$1" ;;
    esac
  }

  # Format a byte count as a human-readable size (keeps the sign for deltas).
  human() {
    awk -v b="$1" 'BEGIN {
      s = (b < 0) ? "-" : ""; a = (b < 0) ? -b : b;
      split("B KiB MiB GiB", u, " "); i = 1;
      while (a >= 1024 && i < 4) { a /= 1024; i++ }
      if (i == 1) printf "%s%d %s", s, a, u[i];
      else        printf "%s%.1f %s", s, a, u[i];
    }'
  }

  # No baseline yet (e.g. first run before main has published one): skip, do not fail.
  if [[ ! -s "$baseline_file" ]]; then
    echo "No baseline found; skipping binary size check."
    exit 0
  fi

  {
    echo "## Binary size check"
    echo ""
    echo "Fails when a binary grows more than **+${threshold}%** over the baseline."
    echo ""
    echo "| | Binary | Baseline | Current | Δ | Change |"
    echo "|---|---|---|---|---|---|"
  } >> "$summary"

  while read -r name current; do
    [[ -z "$name" ]] && continue
    baseline=$(awk -v n="$name" '$1 == n { print $2 }' "$baseline_file")

    # New binary with no baseline: report but do not fail.
    if [[ -z "$baseline" ]]; then
      echo "| ⭐ | $(label "$name") | (new) | $(human "$current") | n/a | n/a |" >> "$summary"
      continue
    fi

    # baseline=0 means a corrupt cached entry; dividing by it would abort the whole script.
    if (( baseline <= 0 )); then
      echo "| ⚠️ | $(label "$name") | (invalid: 0 B) | $(human "$current") | n/a | n/a |" >> "$summary"
      echo "::error::Binary '$name' has an invalid cached baseline size of ${baseline}B; skipping comparison for it."
      status=1
      continue
    fi

    delta=$(( current - baseline ))

    # Single awk computation for both, so decision and display can never disagree.
    read -r pretty exceeds < <(awk -v c="$current" -v b="$baseline" -v t="$threshold" 'BEGIN {
      pct = (c - b) * 100.0 / b
      printf "%+.1f%% %d\n", pct, (pct > t) ? 1 : 0
    }')

    # Pick an indicator: red over the limit, green shrinking, grey unchanged, yellow growing.
    if (( exceeds )); then icon="🔴"
    elif (( delta < 0 )); then icon="🟢"
    elif (( delta == 0 )); then icon="⚪"
    else icon="🟡"
    fi

    delta_h=$(human "$delta"); (( delta > 0 )) && delta_h="+${delta_h}"
    echo "| $icon | $(label "$name") | $(human "$baseline") | $(human "$current") | $delta_h | $pretty |" >> "$summary"

    if (( exceeds )); then
      echo "::error::Binary '$name' grew $pretty (baseline ${baseline}B, current ${current}B), exceeds +${threshold}%."
      status=1
    fi
  done < "$current_file"

  {
    echo ""
    echo "Legend: 🟢 smaller · ⚪ unchanged · 🟡 larger (within limit) · 🔴 over +${threshold}% · ⭐ new · ⚠️ invalid baseline"
  } >> "$summary"

  exit $status
}

cmd="${1:?usage: binary-size.sh <measure|measure-local|check> ...}"
shift
case "$cmd" in
  measure)       measure_size "$@" ;;
  measure-local) measure_local_size "$@" ;;
  check)         check_size "$@" ;;
  *) echo "Unknown subcommand: $cmd" >&2; exit 64 ;;
esac
