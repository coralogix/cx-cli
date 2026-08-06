"""
Automated replay for the `alerts suppression-rules` command group (PR176
console-link verification).

Ground truth: OLD_DIR/results/suppression-rules.jsonl (12 entries). Ledger:

  - create: PASS, known-good payload (supp_test.json shape). AUTOMATED.
  - list / get: FAIL on every format, but the FAIL is already pinned to a
    known, fully-diagnosed pre-existing bug unrelated to PR176 -- id/name/
    description/enabled/created_at/updated_at all deserialize null even
    though the rule genuinely exists (confirmed via the create response and
    a later successful delete of the same id). Exit code is 0 regardless.
    Because the root cause is already understood and documented, replaying
    these calls needs no fresh judgment -- AUTOMATED, with a small
    deterministic parse-and-compare check (no LLM involved) that flags
    whether the bug is still present or has been silently fixed.
  - update: FAIL, and unlike list/get there is NO known-working payload --
    11 payload shape variations were tried in the original session and all
    failed with a path-less "Invalid UUID format" error from the backend.
    Not automatable (nothing "known-good" to replay). See ../manual/
    suppression-rules.md.
  - delete: PASS (used for cleanup of both the "official" and an earlier
    ad-hoc-probe rule). AUTOMATED as part of this script's own cleanup.

This module creates its own throwaway suppression rule (unique name per run,
schedule window computed relative to "now" so it never goes stale), exercises
list/get across text/json/agents, and deletes the rule in a `finally` block.
Deletion tolerates "already gone".
"""

import datetime
import json
import os
import sys
import uuid

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PAYLOADS_DIR = os.path.join(BASE_DIR, "payloads")
TEMPLATE_PATH = os.path.join(PAYLOADS_DIR, "suppression-rules_supp_test.json")

GROUP = "suppression-rules"
CMD_PREFIX = ["alerts", "suppression-rules"]

KNOWN_BUG_NOTE = (
    "Known pre-existing bug (unrelated to PR176): id/name/description/enabled/"
    "created_at/updated_at previously deserialized as null/empty on this call even "
    "though the rule genuinely exists, with exit code 0 (silent data-mapping bug, "
    "not a crash). See OLD_DIR/results/suppression-rules.jsonl 'list'/'get' entries "
    "for the full original diagnosis."
)


def _make_rule_payload(suffix):
    with open(TEMPLATE_PATH) as f:
        body = json.load(f)
    now = datetime.datetime.now(datetime.timezone.utc)
    start = now + datetime.timedelta(hours=1)
    end = now + datetime.timedelta(hours=2)
    rule = body["alertSchedulerRule"]
    rule["name"] = f"pr176-smoke-test-{suffix}"
    rule["schedule"]["oneTime"]["timeframe"]["startTime"] = start.strftime("%Y-%m-%dT%H:%M:%S")
    rule["schedule"]["oneTime"]["timeframe"]["endTime"] = end.strftime("%Y-%m-%dT%H:%M:%S")
    tmp_path = os.path.join(PAYLOADS_DIR, f"_run_suppression-rules_{suffix}.json")
    with open(tmp_path, "w") as f:
        json.dump(body, f)
    return tmp_path, rule["name"]


def _extract_id(stdout):
    try:
        data = json.loads(stdout)
        obj = data[0] if isinstance(data, list) else data
        return obj.get("id")
    except Exception:
        return None


def _check_bug_reproduced(stdout, expected_name):
    """Deterministic (no-LLM) check: does the known null-deserialization bug
    still reproduce? Returns True (bug present), False (looks fixed), or
    None (couldn't parse -- e.g. text/agents format, not judged)."""
    try:
        data = json.loads(stdout)
    except Exception:
        return None
    obj = data[0] if isinstance(data, list) and data else (data if isinstance(data, dict) else None)
    if obj is None:
        return None
    return obj.get("name") != expected_name


def run():
    suffix = uuid.uuid4().hex[:8]
    tmp_payload, rule_name = _make_rule_payload(suffix)
    rule_id = None

    try:
        r = run_cx(
            CMD_PREFIX + ["create", "--from-file", tmp_payload],
            output_format="json",
            extra_flags=["--yes"],
        )
        record(
            GROUP,
            "create (setup)",
            "n/a",
            r,
            notes="setup call creates a real throwaway suppression rule; consoleUrl expected to point to /suppression-rules?edit={id}",
        )
        if r["exit_code"] == 0:
            rule_id = _extract_id(r["stdout"])

        if not rule_id:
            print("suppression-rules.run(): create did not return a usable id -- aborting rest of sequence")
            return

        for fmt in ["text", "json", "agents"]:
            r = run_cx(CMD_PREFIX + ["list"], output_format=fmt)
            bug = _check_bug_reproduced(r["stdout"], rule_name) if fmt == "json" else None
            if bug is True:
                record(GROUP, "list", fmt, r, status="FAIL", notes=KNOWN_BUG_NOTE)
            elif bug is False:
                record(
                    GROUP,
                    "list",
                    fmt,
                    r,
                    status="PASS",
                    notes="Fields matched expected value -- previously-known null-deserialization bug appears FIXED. Worth a manual re-check.",
                )
            else:
                record(GROUP, "list", fmt, r)

        for fmt in ["text", "json", "agents"]:
            r = run_cx(CMD_PREFIX + ["get", rule_id], output_format=fmt)
            bug = _check_bug_reproduced(r["stdout"], rule_name) if fmt == "json" else None
            if bug is True:
                record(GROUP, "get", fmt, r, status="FAIL", notes=KNOWN_BUG_NOTE)
            elif bug is False:
                record(
                    GROUP,
                    "get",
                    fmt,
                    r,
                    status="PASS",
                    notes="Fields matched expected value -- previously-known null-deserialization bug appears FIXED. Worth a manual re-check.",
                )
            else:
                record(GROUP, "get", fmt, r)

        # NOTE: `update` is intentionally NOT replayed here -- no known-working
        # payload was ever found for it. See ../manual/suppression-rules.md.
    finally:
        if rule_id:
            r = run_cx(CMD_PREFIX + ["delete", rule_id], extra_flags=["--yes"])
            record(
                GROUP,
                "delete (cleanup)",
                "n/a",
                r,
                notes="cleanup call; a non-zero exit here (e.g. target already gone) is tolerated as non-fatal",
            )
        try:
            os.remove(tmp_payload)
        except OSError:
            pass


if __name__ == "__main__":
    run()
