"""
Automated replay for the `alerts` command group (PR176 console-link verification).

Ground truth: OLD_DIR/results/alerts.jsonl (20 entries, all PASS). The original
sequence was: create a throwaway alert -> get/enable/disable/list/events/
event-stats across text/json/agents -> delete. Every one of those calls
succeeded cleanly last time with a known-good payload, so the whole group is
AUTOMATED. See ../manual/alerts.md (there is nothing in it -- this group has
zero MANUAL items).

This module creates its own throwaway alert (unique name per run so reruns
never collide with leftovers), exercises every subcommand across the same
output-format matrix as before, and deletes the alert in a `finally` block
so a crash mid-run can't orphan it. Deletion tolerates "already gone".
"""

import json
import os
import sys
import uuid

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PAYLOADS_DIR = os.path.join(BASE_DIR, "payloads")
TEMPLATE_PATH = os.path.join(PAYLOADS_DIR, "alerts_alert_def.json")


def _make_alert_payload(suffix):
    with open(TEMPLATE_PATH) as f:
        body = json.load(f)
    body["alertDefProperties"]["name"] = f"PR176 console-link smoke test {suffix}"
    tmp_path = os.path.join(PAYLOADS_DIR, f"_run_alerts_{suffix}.json")
    with open(tmp_path, "w") as f:
        json.dump(body, f)
    return tmp_path


def _extract_id(stdout):
    try:
        data = json.loads(stdout)
        obj = data[0] if isinstance(data, list) else data
        return obj.get("id")
    except Exception:
        return None


def run():
    suffix = uuid.uuid4().hex[:8]
    tmp_payload = _make_alert_payload(suffix)
    alert_id = None

    try:
        r = run_cx(
            ["alerts", "create", "--from-file", tmp_payload],
            output_format="json",
            extra_flags=["--yes"],
        )
        record(
            "alerts",
            "create (setup)",
            "n/a",
            r,
            notes="setup call to capture alert id; checking for consoleUrl in output",
        )
        if r["exit_code"] == 0:
            alert_id = _extract_id(r["stdout"])

        if not alert_id:
            print("alerts.run(): create did not return a usable id -- aborting rest of sequence")
            return

        id_bound_calls = [
            ("get", ["alerts", "get", alert_id], None),
            ("enable", ["alerts", "enable", alert_id], ["--yes"]),
            ("disable", ["alerts", "disable", alert_id], ["--yes"]),
        ]
        team_wide_calls = [
            ("list", ["alerts", "list"], None),
            ("events", ["alerts", "events"], None),
            ("event-stats", ["alerts", "event-stats"], None),
        ]

        for sub, args, extra in id_bound_calls + team_wide_calls:
            for fmt in ["text", "json", "agents"]:
                r = run_cx(args, output_format=fmt, extra_flags=extra)
                record("alerts", sub, fmt, r)
    finally:
        if alert_id:
            r = run_cx(["alerts", "delete", alert_id], extra_flags=["--yes"])
            record(
                "alerts",
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
