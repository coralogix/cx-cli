"""
Deterministic replay of the `cx logs` console-link verification (PR #176).

Read-only DataPrime log query against the live `kb-demo` team. All 5 entries
in OLD_DIR/results/logs.jsonl are PASS using the same trivial query
(`filter true | limit 5`, single positional QUERY argument) over the last 24h:
the 3-format matrix, one explicit `--tier frequent` run, and one explicit
default-tier (no --tier flag) run -- the latter two use the identical text
command from the matrix, just recorded separately to pin tier behavior.
Nothing is created or mutated; nothing to clean up.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

GROUP = "logs"
FORMATS = ["text", "json", "agents"]
QUERY = "filter true | limit 5"


def run():
    # --- 3-format matrix, default tier ---
    for fmt in FORMATS:
        r = run_cx(["logs", QUERY, "--start", "now-24h"], output_format=fmt)
        record(GROUP, "logs (filter true | limit 5)", fmt, r)

    # --- explicit --tier frequent (text) ---
    r = run_cx(
        ["logs", QUERY, "--start", "now-24h", "--tier", "frequent"],
        output_format="text",
    )
    record(GROUP, "logs --tier frequent", "text", r, notes="explicit frequent tier")

    # --- explicit default tier, no --tier flag (text) ---
    r = run_cx(["logs", QUERY, "--start", "now-24h"], output_format="text")
    record(GROUP, "logs (default tier)", "text", r, notes="default tier (no --tier flag)")


if __name__ == "__main__":
    run()
