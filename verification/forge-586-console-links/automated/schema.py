"""
Deterministic replay of the `cx schema` console-link verification (PR #176).

`schema` is a pure, read-only, no-argument command (dumps the full command
tree as JSON for agent consumption) that never touches the Coralogix API and
carries no console link. The original run (OLD_DIR/results/schema.jsonl) has
exactly 3 entries -- one per output format -- all PASS. Nothing to create,
nothing to clean up.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

GROUP = "schema"
FORMATS = ["text", "json", "agents"]


def run():
    for fmt in FORMATS:
        r = run_cx(["schema"], output_format=fmt)
        record(GROUP, "schema", fmt, r)


if __name__ == "__main__":
    run()
