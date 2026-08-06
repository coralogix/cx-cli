"""
Deterministic replay of the `cx dataprime` console-link verification
(PR #176).

All 9 entries in OLD_DIR/results/dataprime.jsonl are PASS, read-only:

  - `dataprime list`                          -- local reference data, 3 formats
  - `dataprime show aggregate`                -- local reference data, 3 formats
  - `dataprime query 'source logs | limit 5'` -- live query, frequent tier, 3 formats

Nothing is created or mutated; nothing to clean up.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

GROUP = "dataprime"
FORMATS = ["text", "json", "agents"]


def run():
    for fmt in FORMATS:
        r = run_cx(["dataprime", "list"], output_format=fmt)
        record(GROUP, "list", fmt, r)

    for fmt in FORMATS:
        r = run_cx(["dataprime", "show", "aggregate"], output_format=fmt)
        record(GROUP, "show aggregate", fmt, r)

    for fmt in FORMATS:
        r = run_cx(
            [
                "dataprime",
                "query",
                "source logs | limit 5",
                "--start",
                "now-24h",
                "--tier",
                "frequent",
            ],
            output_format=fmt,
        )
        record(GROUP, "query 'source logs | limit 5'", fmt, r)


if __name__ == "__main__":
    run()
