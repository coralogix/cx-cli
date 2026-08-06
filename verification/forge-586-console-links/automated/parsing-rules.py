"""
Automated (deterministic, no-LLM) replay of the console-link verification for `cx parsing-rules`.

Source of truth: OLD_DIR/results/parsing-rules.jsonl + OLD_DIR/run_parsing_rules.py.

Covers (known-working shape from the original run):
  create -> get[text/json/agents] -> update[text/json/agents] -> list[text/json/agents]
  -> usage-limits[text/json/agents] -> delete (cleanup)

NOT covered here (see ../manual/parsing-rules.md):
  bulk-delete - FAILed last time with a real 501 Unimplemented from the backend (known,
  stable baseline), so it is not useful to keep re-running as a mechanical pass/fail check.

Each run creates a rule group with a fresh unique name suffix (uuid4 short hex) so repeated
runs never collide, and deletes it again at the end regardless of outcome (tolerating
"already gone").
"""

import json
import os
import re
import sys
import uuid

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record

GROUP = "parsing-rules"
HERE = os.path.dirname(os.path.abspath(__file__))
BASE_DIR = os.path.dirname(HERE)
PAYLOAD_DIR = os.path.join(BASE_DIR, "payloads")
TEMPLATE = os.path.join(PAYLOAD_DIR, "parsing-rules_create.json")

UUID_RE = re.compile(r"([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})")


def _write_temp(body, name):
    path = os.path.join(PAYLOAD_DIR, name)
    with open(path, "w") as f:
        json.dump(body, f, indent=2)
    return path


def run():
    suffix = uuid.uuid4().hex[:8]

    with open(TEMPLATE) as f:
        create_body = json.load(f)
    create_body["name"] = f"cx-cli-pr176-auto-rule-group-{suffix}"
    create_body["ruleSubgroups"][0]["rules"][0]["name"] = f"cx-cli-pr176-auto-rule-{suffix}"
    create_path = _write_temp(create_body, f"parsing-rules_create_{suffix}.json")

    rg_id = None
    try:
        res = run_cx(["parsing-rules", "create", "--from-file", create_path], output_format=None, extra_flags=["--yes"])
        record(GROUP, "create (setup)", "n/a", res)
        if res["exit_code"] == 0:
            m = UUID_RE.search(res["stdout"]) or UUID_RE.search(res["stderr"])
            if m:
                rg_id = m.group(1)

        if not rg_id:
            print(f"[{GROUP}] ABORT: no id captured from create; skipping rest of lifecycle")
            return

        for fmt in ["text", "json", "agents"]:
            res = run_cx(["parsing-rules", "get", rg_id], output_format=fmt)
            record(GROUP, "get", fmt, res)

        update_body = dict(create_body)
        update_body["description"] = (
            "Temporary automated test rule group for PR176 console-link verification (UPDATED) - safe to delete"
        )
        update_path = _write_temp(update_body, f"parsing-rules_update_{suffix}.json")
        for fmt in ["text", "json", "agents"]:
            res = run_cx(
                ["parsing-rules", "update", "--from-file", update_path, rg_id],
                output_format=fmt,
                extra_flags=["--yes"],
            )
            record(GROUP, "update", fmt, res)

        for fmt in ["text", "json", "agents"]:
            res = run_cx(["parsing-rules", "list"], output_format=fmt)
            record(GROUP, "list", fmt, res)

        for fmt in ["text", "json", "agents"]:
            res = run_cx(["parsing-rules", "usage-limits"], output_format=fmt)
            record(GROUP, "usage-limits", fmt, res)
    finally:
        if rg_id:
            res = run_cx(["parsing-rules", "delete", rg_id], output_format=None, extra_flags=["--yes"])
            ok = res["exit_code"] == 0 or "not found" in (res.get("stderr") or "").lower() or "404" in (res.get("stderr") or "")
            record(GROUP, "delete (cleanup)", "n/a", res, status="PASS" if ok else None,
                   notes="" if res["exit_code"] == 0 else "tolerated: resource already gone")
        for p in (create_path, locals().get("update_path")):
            if p and os.path.exists(p):
                try:
                    os.remove(p)
                except OSError:
                    pass

    print(f"[{GROUP}] done")


if __name__ == "__main__":
    run()
