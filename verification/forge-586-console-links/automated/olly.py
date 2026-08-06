"""
Automated (deterministic, no-LLM) replay of the console-link verification for `cx olly`.

Source of truth: OLD_DIR/results/olly.jsonl + OLD_DIR/step9_olly.py, step10_olly.py,
step11_olly.py.

Covers:
  - `artifacts list` - pure read-only, PASS last time in all three formats.
  - `artifacts get <id>` - PASS last time using a real, pre-existing artifact id from this
    team (30fbcbe6-a104-4a00-bde3-03d6b3b1d693). This is read-only (a GET) and does not
    mutate anything, so replaying it against the same known-good id is mechanical -
    olly-generated artifacts are not expected to be deleted out from under this team in
    normal operation.

NOT covered here (see ../manual/olly.md):
  `ask` - a real call to the Coralogix AI assistant. Every invocation costs real tokens,
  produces a non-deterministic response, and (per the old run) returns an artifact-like
  link in its response that does NOT correspond to a real `artifacts get` id (it 404s) -
  interpreting a fresh `ask` response requires judgment, not mechanical replay, and
  should not be run automatically/repeatedly just to regenerate this test data.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record

GROUP = "olly"
KNOWN_ARTIFACT_ID = "30fbcbe6-a104-4a00-bde3-03d6b3b1d693"


def run():
    for fmt in ["text", "json", "agents"]:
        res = run_cx(["olly", "artifacts", "list"], output_format=fmt)
        record(GROUP, "artifacts list", fmt, res)

    for fmt in ["text", "json", "agents"]:
        res = run_cx(["olly", "artifacts", "get", KNOWN_ARTIFACT_ID], output_format=fmt)
        record(GROUP, "artifacts get (real pre-existing artifact id)", fmt, res)

    print(f"[{GROUP}] done")


if __name__ == "__main__":
    run()
