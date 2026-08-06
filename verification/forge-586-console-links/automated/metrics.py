"""
Deterministic replay of the `cx metrics` console-link verification (PR #176).

Read-only PromQL queries against the live `kb-demo` team. All 12 entries in
OLD_DIR/results/metrics.jsonl are PASS, using a metric
(`container_memory_usage_By`) confirmed to exist on this team:

  - `query <metric>`               -- instant query, 3 formats
  - `query-range <metric> ...`     -- range query, 3 formats
  - `search --name container_memory*` -- metric name search, 3 formats
  - `get-labels <metric>`          -- label names, 3 formats

Nothing is created or mutated; nothing to clean up.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

GROUP = "metrics"
FORMATS = ["text", "json", "agents"]
METRIC = "container_memory_usage_By"


def run():
    for fmt in FORMATS:
        r = run_cx(["metrics", "query", METRIC], output_format=fmt)
        record(GROUP, f"query {METRIC}", fmt, r)

    for fmt in FORMATS:
        r = run_cx(
            [
                "metrics",
                "query-range",
                METRIC,
                "--start",
                "now-1h",
                "--end",
                "now",
                "--step",
                "5m",
            ],
            output_format=fmt,
        )
        record(GROUP, f"query-range {METRIC}", fmt, r)

    for fmt in FORMATS:
        r = run_cx(["metrics", "search", "--name", "container_memory*"], output_format=fmt)
        record(GROUP, "search --name container_memory*", fmt, r)

    for fmt in FORMATS:
        r = run_cx(["metrics", "get-labels", METRIC], output_format=fmt)
        record(GROUP, f"get-labels {METRIC}", fmt, r)


if __name__ == "__main__":
    run()
