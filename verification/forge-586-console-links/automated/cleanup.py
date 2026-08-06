"""
Deterministic replay of the `cx cleanup` console-link verification (PR #176).

`cleanup` is a local, no-argument, idempotent housekeeping command ("Remove
stale cx_results* files (older than 30 minutes) from the temp directory") --
no Coralogix API calls, no console link, and safe to run any number of times
(it only ever deletes files matching its own spill-file naming convention
that are already 30+ minutes old). The original run
(OLD_DIR/results/cleanup.jsonl) has exactly 3 entries -- one per output
format -- all PASS. Nothing to create, nothing to clean up beyond what the
command itself does.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

GROUP = "cleanup"
FORMATS = ["text", "json", "agents"]


def run():
    for fmt in FORMATS:
        r = run_cx(["cleanup"], output_format=fmt)
        record(GROUP, "cleanup", fmt, r)


if __name__ == "__main__":
    run()
