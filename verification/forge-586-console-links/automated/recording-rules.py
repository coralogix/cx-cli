"""
Automated (deterministic, no-LLM) replay of the console-link verification for `cx recording-rules`.

Source of truth: OLD_DIR/results/recording-rules.jsonl + OLD_DIR/run_recording_rules.py,
run_recording_rules2.py.

Only `list` is replayed here - it is the one subcommand that behaved as a normal read-only
call last time (PASS, no side effects).

NOT covered here (see ../manual/recording-rules.md):
  `create`, `get`, `update`, `delete` - the old run found that `recording-rules create`
  returns exit 0 / HTTP 200 (with the corrected `{"groups": [...]}` + integer-seconds
  `interval` schema), but the created group then never appears via `list` or `get` by any
  ID, even after a 25s wait. `get`/`update`/`delete` were SKIPPED entirely because there
  was no valid ID to test them with. This means we cannot confirm whether `create`
  actually leaves a persistent (if invisible) resource behind, and there is no confirmed
  working delete route - exactly the "no confirmed delete route" case that must not be
  automated.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record

GROUP = "recording-rules"


def run():
    for fmt in ["text", "json", "agents"]:
        res = run_cx(["recording-rules", "list"], output_format=fmt)
        record(GROUP, "list", fmt, res)

    print(f"[{GROUP}] done")


if __name__ == "__main__":
    run()
