"""
Deterministic replay of the `cx search-fields` console-link verification
(PR #176).

All 6 entries in OLD_DIR/results/search-fields.jsonl are PASS, read-only
Olly Knowledge Base semantic-search calls against the live `kb-demo` team:

  - `search-fields "http response status code" --dataset logs`       -- semantic (default), 3 formats
  - `search-fields payment -s value --dataset all --limit 5`         -- value search, 3 formats

Nothing is created or mutated; nothing to clean up.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

GROUP = "search-fields"
FORMATS = ["text", "json", "agents"]


def run():
    for fmt in FORMATS:
        r = run_cx(
            [
                "search-fields",
                "http response status code",
                "--dataset",
                "logs",
                "--limit",
                "5",
            ],
            output_format=fmt,
        )
        record(GROUP, "semantic 'http response status code' --dataset logs", fmt, r)

    for fmt in FORMATS:
        r = run_cx(
            [
                "search-fields",
                "payment",
                "-s",
                "value",
                "--dataset",
                "all",
                "--limit",
                "5",
            ],
            output_format=fmt,
        )
        record(GROUP, "value 'payment' --dataset all", fmt, r)


if __name__ == "__main__":
    run()
