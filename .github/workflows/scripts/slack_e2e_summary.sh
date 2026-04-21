#!/bin/bash
# Sends a nightly e2e test summary table to Slack via an incoming webhook.
#
# Required environment variables:
#   SLACK_WEBHOOK_URL  - Slack incoming webhook URL
#   RUN_URL            - GitHub Actions run URL shown in the "View run" link
#
# Optional environment variables:
#   E2E_REPORT_TITLE   - Custom title prefix (default: "Agent Control Nightly E2E Results")
#
# Expects per-scenario TSV result files at e2e-results/e2e-result-{env}-{scenario}.txt
# (written by report_e2e_result.sh), each with the format:
#   environment<TAB>scenario<TAB>duration<TAB>status

set -euo pipefail

# Prepend the header line to the sorted scenario results.
# cat gives a stable alphabetical order using file name using file names.
tsv=$(
  printf "NR Account\tPlatform\tScenario\tDuration\tStatus\n"
  cat e2e-results/*.txt
)

# Compute the title: show failure count if any, otherwise confirm all passed.
total=$(tail -n +2 <<< "$tsv" | wc -l | xargs)
failures=$(tail -n +2 <<< "$tsv" | grep -cF $'\t❌ Failure' || true)
title_prefix="${E2E_REPORT_TITLE:-Agent Control Nightly E2E Results}"
if (( failures > 0 )); then
  title="❌ ${title_prefix}: ${failures}/${total} failed"
else
  title="✅ ${title_prefix}"
fi

# Build detailed test results from JSON files (if present)
# Use jq to build the entire structure to handle escaping correctly
details_blocks_json="null"
json_file="e2e-results/fleet-control-test-report.json"
if [ -f "$json_file" ]; then
  details_blocks_json=$(jq -c '
    # Build summary line
    ("✅ " + (.totalPassed | tostring) + " passed  ❌ " + (.totalFailed | tostring) + " failed  ⚠️ " + (.totalInconclusive | tostring) + " inconclusive  ⏭️ " + (.totalIgnored | tostring) + " ignored") as $summary |

    # Build blocks array
    [
      {type: "divider"},
      {type: "section", text: {type: "mrkdwn", text: "*📊 Fleet Control Test Details*"}},
      {type: "section", text: {type: "mrkdwn", text: $summary}}
    ] +
    # Add test name blocks in order: failed, inconclusive, passed, ignored
    (if .totalFailed > 0 then
      [{type: "section", text: {type: "mrkdwn", text: ("*❌ Failed tests:*\n" + ([.failedTests | to_entries[] | "  *[\(.key)]*\n" + (.value | map("    • " + .) | join("\n"))] | join("\n")))}}]
    else [] end) +
    (if .totalInconclusive > 0 then
      [{type: "section", text: {type: "mrkdwn", text: ("*⚠️ Inconclusive tests:*\n" + ([.inconclusiveTests | to_entries[] | "  *[\(.key)]*\n" + (.value | map("    • " + .) | join("\n"))] | join("\n")))}}]
    else [] end) +
    (if .totalPassed > 0 then
      [{type: "section", text: {type: "mrkdwn", text: ("*✅ Passed tests:*\n" + ([.passedTests | to_entries[] | "  *[\(.key)]*\n" + (.value | map("    • " + .) | join("\n"))] | join("\n")))}}]
    else [] end) +
    (if .totalIgnored > 0 then
      [{type: "section", text: {type: "mrkdwn", text: ("*⏭️ Ignored tests:*\n" + ([.ignoredTests | to_entries[] | "  *[\(.key)]*\n" + (.value | map("    • " + .) | join("\n"))] | join("\n")))}}]
    else [] end)
  ' "$json_file")
fi

# Build the Slack Block Kit payload from the TSV.
#
# jq -Rs reads stdin as a single raw string (-R = no JSON parsing, -s = slurp).
# Inside the filter:
#   1. Parse: split the string by newlines then tabs to get a 2D array $rows,
#      where $rows[0] is the header row and $rows[1:] are the data rows.
#   2. Convert: map each cell string to a Slack rich_text block. Header cells
#      are bold; data cells are plain.
#   3. Assemble: build the Block Kit payload with a header block, a table block
#      whose "rows" field is a 2D array of rich_text cells, optional detail blocks
#      (from JSON reports), and a context block with the run URL.
payload=$(printf '%s' "$tsv" | jq -Rs \
  --arg url   "$RUN_URL" \
  --arg title "$title" \
  --argjson details_blocks "$details_blocks_json" \
  '
  [split("\n") | .[] | select(length > 0) | split("\t")] as $rows |

  def plain_cell: {type: "rich_text", elements: [{type: "rich_text_section", elements: [{type: "text", text: .}]}]};
  def bold_cell:  {type: "rich_text", elements: [{type: "rich_text_section", elements: [{type: "text", text: ., style: {bold: true}}]}]};

  ($rows[0]  | map(bold_cell))       as $header_row |
  ($rows[1:] | map(map(plain_cell))) as $data_rows  |

  # Use details blocks if present (already parsed JSON array)
  ($details_blocks // []) as $detail_blocks |

  {
    blocks: ([
      {type: "header",  text: {type: "plain_text", text: $title}},
      {type: "table",   rows: ([$header_row] + $data_rows)}
    ] + $detail_blocks + [
      {type: "context", elements: [{type: "mrkdwn", text: ":github: <\($url)|Workflow Run> :nr-logo_green5: <https://onenr.io/0Zw09VM4eRv|Dashboard>"}]}
    ])
  }
  '
)

curl -s -X POST "$SLACK_WEBHOOK_URL" \
  -H "Content-Type: application/json" \
  -d "$payload"
