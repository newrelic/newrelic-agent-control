#!/bin/bash
# Sends a nightly e2e test summary table to Slack via an incoming webhook.
#
# Required environment variables:
#   SLACK_WEBHOOK_URL  - Slack incoming webhook URL
#   RUN_URL            - GitHub Actions run URL shown in the "View run" link
#
# Expects per-scenario TSV result files at e2e-results/e2e-result-{env}-{scenario}.txt
# (written by report_e2e_result.sh), each with the format:
#   environment<TAB>scenario<TAB>duration<TAB>status

set -euo pipefail

# Prepend the header line to the sorted scenario results.
# sort gives a stable alphabetical order across environments and scenarios.
tsv=$(
  printf "NR Account\tPlatform\tScenario\tDuration\tStatus\n"
  sort e2e-results/*.txt
)

# Build the Slack Block Kit payload from the TSV.
#
# jq -Rs reads stdin as a single raw string (-R = no JSON parsing, -s = slurp).
# Inside the filter:
#   1. Parse: split the string by newlines then tabs to get a 2D array $rows,
#      where $rows[0] is the header row and $rows[1:] are the data rows.
#   2. Convert: map each cell string to a Slack rich_text block. Header cells
#      are bold; data cells are plain.
#   3. Assemble: build the Block Kit payload with a header block, a table block
#      whose "rows" field is a 2D array of rich_text cells, and a context block
#      with the run URL.
payload=$(printf '%s' "$tsv" | jq -Rs \
  --arg url "$RUN_URL" \
  '
  [split("\n") | .[] | select(length > 0) | split("\t")] as $rows |

  def plain_cell: {type: "rich_text", elements: [{type: "rich_text_section", elements: [{type: "text", text: .}]}]};
  def bold_cell:  {type: "rich_text", elements: [{type: "rich_text_section", elements: [{type: "text", text: ., style: {bold: true}}]}]};

  ($rows[0]  | map(bold_cell))       as $header_row |
  ($rows[1:] | map(map(plain_cell))) as $data_rows  |

  {
    blocks: [
      {type: "header",  text: {type: "plain_text", text: "🧪 Agent Control Nightly E2E Results"}},
      {type: "table",   rows: ([$header_row] + $data_rows)},
      {type: "context", elements: [{type: "mrkdwn", text: (":github: <" + $url + "|Workflow Run>")}]}
    ]
  }
  '
)

curl -s -X POST "$SLACK_WEBHOOK_URL" \
  -H "Content-Type: application/json" \
  -d "$payload"
