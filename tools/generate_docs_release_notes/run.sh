#!/bin/bash
# Generates a docs-website MDX release notes file for Agent Control.
#
# Usage: run.sh <CHANGELOG_MD_PATH> <VERSION>
#
#   <CHANGELOG_MD_PATH>  CHANGELOG.md. Both the release DATE and the body
#                        SECTIONS are read from this file's
#                        `## v<VERSION> - <YYYY-MM-DD>` section (the toolkit's
#                        `update-markdown` step writes the heading + body
#                        together, so no separate partial file is needed).
#   <VERSION>            Bare version, no `v` prefix (e.g. 1.99.0).
#
# Writes `agent-control-<YYYY-MM-DD>.mdx` (release date) to the current
# directory and prints the output path on success.
#
# Requires: python3.

set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "Usage: $0 <CHANGELOG_MD_PATH> <VERSION>" >&2
  exit 1
fi

CHANGELOG_MD_PATH="$1"
VERSION="$2"

if [ ! -f "$CHANGELOG_MD_PATH" ]; then
  echo "Error: changelog file not found: $CHANGELOG_MD_PATH" >&2
  exit 1
fi

export CHANGELOG_MD_PATH VERSION

python3 << 'PYEOF'
import os
import re
import sys

changelog_md_path = os.environ['CHANGELOG_MD_PATH']
version           = os.environ['VERSION']

with open(changelog_md_path, encoding='utf-8') as f:
    changelog = f.read().replace('\r', '')

# Slice the target version's section: from its `## v<VERSION> - <date>` heading
# up to the next `## ` heading (or EOF), so sibling versions cannot leak in.
section_pattern = (
    rf'^##\s+v{re.escape(version)}'
    rf'(?:\s+-\s+(\d{{4}}-\d{{2}}-\d{{2}}))?\s*$\n'
    rf'(.*?)(?=^##\s+v|\Z)'
)
section_match = re.search(section_pattern, changelog, re.DOTALL | re.MULTILINE)
if not section_match:
    sys.stderr.write(
        f"Error: could not find heading '## v{version}' in {changelog_md_path}\n"
    )
    sys.exit(1)

release_date = section_match.group(1)
if not release_date:
    sys.stderr.write(
        f"Error: heading '## v{version}' in {changelog_md_path} has no "
        f"'- <YYYY-MM-DD>' date\n"
    )
    sys.exit(1)

body = section_match.group(2)


def extract_section(text, *keywords):
    """Return cleaned bullets from the first `### ...` section whose heading
    contains one of `keywords` (case-insensitive, emoji ignored)."""
    # Split the body into `### ...` sections.
    parts = re.split(r'^###\s+(.*)$', text, flags=re.MULTILINE)
    # parts[0] is any preamble; then alternating (heading, content).
    for i in range(1, len(parts), 2):
        heading = parts[i]
        content = parts[i + 1] if i + 1 < len(parts) else ''
        heading_lower = heading.lower()
        if not any(kw.lower() in heading_lower for kw in keywords):
            continue
        items = []
        for line in content.splitlines():
            line = line.strip()
            if not (line.startswith('* ') or line.startswith('- ')):
                continue
            item = line[2:].strip()
            # Strip a single trailing PR ref `(#123)` or commit ref `(abc1234)`.
            item = re.sub(r'\s*\(#\d+\)\s*$', '', item)
            item = re.sub(r'\s*\([0-9a-fA-F]{7,40}\)\s*$', '', item)
            item = item.strip()
            if item:
                items.append(item)
        return items
    return []


features = extract_section(body, 'Enhancements', 'Features')
bugs     = extract_section(body, 'Bug fixes', 'Fixes', 'Bugfixes')
security = extract_section(body, 'Security notices', 'Security')


def yaml_list(items):
    escaped = ["'" + item.replace("'", "''") + "'" for item in items]
    return '[' + ', '.join(escaped) + ']'


output_file = f'agent-control-{release_date}.mdx'

lines = [
    '---',
    'subject: Agent Control',
    f"releaseDate: '{release_date}'",
    f'version: {version}',
]
if features:
    lines.append(f'features: {yaml_list(features)}')
if bugs:
    lines.append(f'bugs: {yaml_list(bugs)}')
if security:
    lines.append(f'security: {yaml_list(security)}')
lines.append('---')

release_url = (
    f'https://github.com/newrelic/newrelic-super-agent/releases/tag/{version}'
)

def markdown_section(title, items):
    if not items:
        return ''
    bullets = '\n'.join(f'- {item}' for item in items)
    return f'## {title}\n\n{bullets}\n\n'


content = '\n'.join(lines) + '\n\n'
content += markdown_section('Features', features)
content += markdown_section('Fixes', bugs)
content += markdown_section('Security', security)
content += (
    'For a detailed description of changes, see the '
    f'[release notes]({release_url}).\n'
)

with open(output_file, 'w', encoding='utf-8') as f:
    f.write(content)

print(output_file)
PYEOF
