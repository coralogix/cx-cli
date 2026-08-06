"""
Automated (deterministic, no-LLM) replay of the console-link verification for `cx retentions`.

Source of truth: OLD_DIR/results/retentions.jsonl + OLD_DIR/run_retentions_baseline.py,
run_retentions.py.

Only the two read-only subcommands are replayed here: `list` and `status`.

NOT covered here (see ../manual/retentions.md):
  `activate` and `update` - the old run's `activate` call flipped this live team's
  `enableTags` setting from false to true, and `update` (the only exposed way to try to
  revert it) returns a real, stable 501 Unimplemented for this team/plan - so the flip
  could NOT be reverted and is a permanent residual side effect on the "kb-demo" team.
  These must never be auto-replayed again: doing so would either re-flip an already-true
  flag (harmless but pointless) or, on a fresh team without this residue, flip another
  irreversible flag with no way back.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record

GROUP = "retentions"


def run():
    for fmt in ["text", "json", "agents"]:
        res = run_cx(["retentions", "list"], output_format=fmt)
        record(GROUP, "list", fmt, res,
               notes="read-only; no console page exists for retentions (confirmed in the original run) - expect no consoleUrl")

    for fmt in ["text", "json", "agents"]:
        res = run_cx(["retentions", "status"], output_format=fmt)
        record(GROUP, "status", fmt, res)

    print(f"[{GROUP}] done")


if __name__ == "__main__":
    run()
