"""
Deterministic replay of the `cx spans` console-link verification (PR #176).

Read-only DataPrime span query against the live `kb-demo` team. All 3
entries in OLD_DIR/results/spans.jsonl are PASS using the same trivial query
(`filter true | limit 5`, single positional QUERY argument -- `source spans`
is auto-prepended by the CLI) over the last 24h, one per output format.
Nothing is created or mutated; nothing to clean up.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

GROUP = "spans"
FORMATS = ["text", "json", "agents"]
QUERY = "filter true | limit 5"


def run():
    for fmt in FORMATS:
        r = run_cx(["spans", QUERY, "--start", "now-24h"], output_format=fmt)
        record(GROUP, "spans (filter true | limit 5)", fmt, r)


if __name__ == "__main__":
    run()
