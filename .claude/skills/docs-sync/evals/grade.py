#!/usr/bin/env python3
"""Grades docs-sync reports against evals.json.

Usage: grade.py <iteration-dir> [--evals evals.json]

Expects <iteration-dir>/<eval-name>/with_skill/outputs/report.md (the report, or the
"no doc impact" message) plus docs.diff (`git diff` of the fixture after the run) and
status.txt (`git status --short`), and, optionally,
<iteration-dir>/<eval-name>/with_skill/timing.json ({"total_tokens": N}).
Writes <iteration-dir>/<eval-name>/with_skill/grading.json and exits 1 if any check fails.
"""
import argparse
import json
import re
import sys
from pathlib import Path

# Findings appear as chat-style lines ("L42 [misleading] ...") or as report table rows
# ("| 42 | misleading | ... |").
FINDING = re.compile(
    r"^\s*L\d+[\d,\-]*\s+\[(misleading|stale|typo)\]|^\|\s*[\d,\-]+\s*\|\s*(misleading|stale|typo)\s*\|",
    re.M | re.I,
)
UNLABELED_FINDING = re.compile(r"^\s*L\d+[\d,\-]*\s+[\"`]|^\|\s*[\d,\-]+\s*\|\s*[\"`]", re.M)
EVIDENCE = re.compile(
    r"^\s*code:|^\|\s*[\d,\-]+\s*\|\s*(?:misleading|stale|typo)\s*\|[^|]*\|[^|]*`[^`|]+:\d+(?:-\d+)?`",
    re.M | re.I,
)


def run_check(check, report, timing, diff="", status=""):
    kind = check["type"]
    if kind == "diff_touches":
        files = re.findall(r"^\+\+\+ b/(.+)$", diff, re.M)
        hit = [f for f in files if re.search(check["pattern"], f)]
        return bool(hit), f"edited files: {files or 'none'}"
    if kind == "diff_untouched":
        files = re.findall(r"^\+\+\+ b/(.+)$", diff, re.M)
        hit = [f for f in files if re.search(check["pattern"], f)]
        return not hit, f"edited files matching {check['pattern']!r}: {hit or 'none'}"
    if kind == "diff_added_absent":
        added = [l[1:] for l in diff.splitlines() if l.startswith("+") and not l.startswith("+++")]
        bad = [l for l in added if re.search(check["pattern"], l)]
        return not bad, f"{len(bad)} added line(s) match {check['pattern']!r}"
    if kind == "no_edits":
        return not diff.strip(), "no doc edits" if not diff.strip() else "docs were edited"
    if kind == "edits_confined":
        paths = [l[3:].strip() for l in status.splitlines() if l.strip()]
        bad = [p for p in paths if not (p.startswith("docs/") or p == "docs-sync-audit.md")]
        return not bad, f"changed paths outside docs/ and the report: {bad or 'none'}"
    if kind == "matches":
        hit = re.search(check["pattern"], report, re.M | re.I)
        return bool(hit), f"pattern {check['pattern']!r} " + ("found" if hit else "not found")
    if kind == "has_findings":
        n = len(FINDING.findall(report))
        return n >= check.get("min", 1), f"{n} finding(s)"
    if kind == "no_findings":
        n = len(FINDING.findall(report))
        return n == 0, f"{n} finding(s)"
    if kind == "evidence_per_finding":
        f, e = len(FINDING.findall(report)), len(EVIDENCE.findall(report))
        return e >= f, f"{f} finding(s), {e} code evidence line(s)"
    if kind == "severity_per_finding":
        n = len(UNLABELED_FINDING.findall(report))
        return n == 0, f"{n} finding line(s) without a severity label"
    if kind == "max_tokens":
        if timing is None:
            return True, "no timing.json, skipped"
        tokens = timing.get("total_tokens", 0)
        return tokens <= check["value"], f"{tokens} tokens (budget {check['value']})"
    raise ValueError(f"unknown check type {kind!r}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("iteration_dir", type=Path)
    parser.add_argument("--evals", type=Path, default=Path(__file__).with_name("evals.json"))
    args = parser.parse_args()

    spec = json.loads(args.evals.read_text())
    global_checks = spec.get("global_checks", [])
    failed_any = False
    print(f"{'eval':28} {'passed':>8}  failed checks")

    for ev in spec["evals"]:
        run_dir = args.iteration_dir / ev["name"] / "with_skill"
        report_path = run_dir / "outputs" / "report.md"
        if not report_path.exists():
            print(f"{ev['name']:28} {'-':>8}  missing {report_path}")
            failed_any = True
            continue
        report = report_path.read_text()
        timing_path = run_dir / "timing.json"
        timing = json.loads(timing_path.read_text()) if timing_path.exists() else None
        diff_path, status_path = run_dir / "outputs" / "docs.diff", run_dir / "outputs" / "status.txt"
        diff = diff_path.read_text() if diff_path.exists() else ""
        status = status_path.read_text() if status_path.exists() else ""

        expectations = []
        for check in ev.get("checks", []) + global_checks:
            passed, evidence = run_check(check, report, timing, diff, status)
            expectations.append({"text": check["text"], "passed": passed, "evidence": evidence})

        passed_n = sum(e["passed"] for e in expectations)
        summary = {
            "passed": passed_n,
            "failed": len(expectations) - passed_n,
            "total": len(expectations),
            "pass_rate": passed_n / len(expectations) if expectations else 1.0,
        }
        (run_dir / "grading.json").write_text(json.dumps({"expectations": expectations, "summary": summary}, indent=2))

        failures = [e["text"] for e in expectations if not e["passed"]]
        failed_any |= bool(failures)
        print(f"{ev['name']:28} {passed_n:>3}/{len(expectations):<4}  {'; '.join(failures)}")

    sys.exit(1 if failed_any else 0)


if __name__ == "__main__":
    main()
