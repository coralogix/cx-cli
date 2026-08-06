"""
Automated (deterministic, no-LLM) replay of the console-link verification for `cx usage`.

Source of truth: OLD_DIR/results/usage.jsonl + OLD_DIR/run_usage.py.

Every `usage` subcommand tested last time was read-only and PASSed in all three output
formats, so the entire group is replayed here verbatim - nothing to create/clean up.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record

GROUP = "usage"
FORMATS = ("text", "json", "agents")


def run():
    for fmt in FORMATS:
        res = run_cx(["usage", "summary"], output_format=fmt)
        record(GROUP, "summary", fmt, res)

    for fmt in FORMATS:
        res = run_cx(["usage", "daily", "--start", "now-7d", "--end", "now"], output_format=fmt)
        record(GROUP, "daily", fmt, res)

    for fmt in FORMATS:
        res = run_cx(["usage", "logs-count", "--start", "now-24h", "--end", "now"], output_format=fmt)
        record(GROUP, "logs-count", fmt, res)

    for fmt in FORMATS:
        res = run_cx(["usage", "spans-count", "--start", "now-24h", "--end", "now"], output_format=fmt)
        record(GROUP, "spans-count", fmt, res)

    for fmt in FORMATS:
        res = run_cx(["usage", "capabilities"], output_format=fmt)
        record(GROUP, "capabilities", fmt, res)

    for fmt in FORMATS:
        res = run_cx(
            ["usage", "query", "--query", '{"daily":{"relativeRange":"DAILY_RELATIVE_RANGE_LAST_7_DAYS"}}'],
            output_format=fmt,
        )
        record(GROUP, "query", fmt, res)

    for fmt in FORMATS:
        res = run_cx(["usage", "export-status"], output_format=fmt)
        record(GROUP, "export-status", fmt, res)

    print(f"[{GROUP}] done")


if __name__ == "__main__":
    run()
