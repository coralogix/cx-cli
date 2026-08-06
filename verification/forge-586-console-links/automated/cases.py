"""
Automated replay for the `cases` command group (PR176 console-link
verification).

`cases` has no create/list-your-own-throwaway-object of its own -- every
subcommand exercised in the original session (OLD_DIR/results/cases.jsonl,
42 entries) operated on REAL, pre-existing demo-team cases. The session
notes explicitly record that 3 real cases (CASE-43, CASE-96, CASE-97) were
resolved and then closed during that run, with no undo available for the
CLOSED status.

Per the classification rules for this task, ANY subcommand that mutates a
real case's state (title update, comment, assign/unassign, acknowledge/
unacknowledge, set/clear-priority, resolve, close) is MANUAL, not automated:
there is no safe, judgment-free way to keep re-running those against a real
case forever, and resolve/close are partly irreversible. See
../manual/cases.md for the full list with baselines.

Only the four genuinely read-only lookups are replayed here, against the
same stable ids used last time:
  - get               (case id)
  - events list        (case id)
  - events get          (event id)
  - notifications       (case id)

These are safe to mechanically replay indefinitely: they don't change
anything, and the ids point at durable objects (cases + a case event) in the
kb-demo demo team.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

GROUP = "cases"

# Stable ids from the original verification run (OLD_DIR/results/cases.jsonl).
# These are real demo-team objects, not created/owned by this script.
CASE_IDS = [
    "8623c2d0-2d1b-5094-a3b6-d454c112e5d9",
    "b0ea9128-e734-58a3-a160-cf46e357a9ed",
    "ee1dca07-c329-5250-917f-3b0374f86af0",
]
EVENT_ID = "12c5ef38-3ed8-4f7b-bc3a-444c324acbd5"

FORMATS = ["text", "json", "agents"]


def run():
    # get: one call per format, cycling through the three known case ids
    # (mirrors the original 1-format-per-id pattern).
    for fmt, case_id in zip(FORMATS, CASE_IDS):
        r = run_cx(["cases", "get", case_id], output_format=fmt, extra_flags=["--yes"])
        record(GROUP, "get", fmt, r)

    # events list: same case-id-per-format pattern as before.
    for fmt, case_id in zip(FORMATS, CASE_IDS):
        r = run_cx(["cases", "events", "list", case_id], output_format=fmt)
        record(GROUP, "events list", fmt, r)

    # events get: fixed event id across all three formats.
    for fmt in FORMATS:
        r = run_cx(["cases", "events", "get", EVENT_ID], output_format=fmt)
        record(GROUP, "events get", fmt, r)

    # notifications: same case-id-per-format pattern as before.
    for fmt, case_id in zip(FORMATS, CASE_IDS):
        r = run_cx(["cases", "notifications", case_id], output_format=fmt)
        record(GROUP, "notifications", fmt, r)


if __name__ == "__main__":
    run()
