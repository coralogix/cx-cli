"""
Automated (deterministic, no-LLM) replay of the console-link verification for `cx e2m`.

Source of truth: OLD_DIR/results/e2m.jsonl + OLD_DIR/run_e2m.py + OLD_DIR/run_e2m_update_retry.py.

Covers the known-working shape (the first `update` attempt in the old run FAILed because
the test payload wrongly kept the create-only `permutationsLimit` field - the *retry*
with that field popped and `id` set is the confirmed-working shape used here):

  create -> get[text/json/agents] -> update[text/json/agents] (fixed payload)
  -> list[text/json/agents] -> limits[text/json/agents] -> delete (cleanup)

NOT covered here (see ../manual/e2m.md):
  `labels-cardinality` - FAILed with a 400 "query is required" from the backend every
  format last time; the cx CLI sends no body/query params for this GET at all, so this
  looks like a possible real API-contract mismatch worth a human look, not a flaky
  environment issue to just retry.

Each run creates an E2M definition with a fresh unique name suffix so repeated runs never
collide, and deletes it again at the end regardless of outcome (tolerating "already gone").
"""

import json
import os
import re
import sys
import uuid

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record

GROUP = "e2m"
HERE = os.path.dirname(os.path.abspath(__file__))
BASE_DIR = os.path.dirname(HERE)
PAYLOAD_DIR = os.path.join(BASE_DIR, "payloads")
TEMPLATE = os.path.join(PAYLOAD_DIR, "e2m_create.json")

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
    create_body["name"] = f"cx-cli-pr176-auto-e2m-{suffix}"
    create_path = _write_temp(create_body, f"e2m_create_{suffix}.json")

    e2m_id = None
    update_path = None
    try:
        res = run_cx(["e2m", "create", "--from-file", create_path], output_format=None, extra_flags=["--yes"])
        record(GROUP, "create (setup)", "n/a", res)
        if res["exit_code"] == 0:
            m = UUID_RE.search(res["stdout"]) or UUID_RE.search(res["stderr"])
            if m:
                e2m_id = m.group(1)

        if not e2m_id:
            print(f"[{GROUP}] ABORT: no id captured from create; skipping rest of lifecycle")
            return

        for fmt in ["text", "json", "agents"]:
            res = run_cx(["e2m", "get", e2m_id], output_format=fmt)
            record(GROUP, "get", fmt, res)

        # update payload = create payload minus the create-only `permutationsLimit` field,
        # plus the assigned id (this is the corrected shape from run_e2m_update_retry.py)
        update_body = dict(create_body)
        update_body.pop("permutationsLimit", None)
        update_body["id"] = e2m_id
        update_body["description"] = (
            "Temporary automated test E2M for PR176 console-link verification (UPDATED) - safe to delete"
        )
        update_path = _write_temp(update_body, f"e2m_update_{suffix}.json")
        for fmt in ["text", "json", "agents"]:
            res = run_cx(
                ["e2m", "update", "--from-file", update_path],
                output_format=fmt,
                extra_flags=["--yes"],
            )
            record(GROUP, "update (fixed payload)", fmt, res)

        for fmt in ["text", "json", "agents"]:
            res = run_cx(["e2m", "list"], output_format=fmt)
            record(GROUP, "list", fmt, res)

        for fmt in ["text", "json", "agents"]:
            res = run_cx(["e2m", "limits"], output_format=fmt)
            record(GROUP, "limits", fmt, res)
    finally:
        if e2m_id:
            res = run_cx(["e2m", "delete", e2m_id], output_format=None, extra_flags=["--yes"])
            ok = res["exit_code"] == 0 or "not found" in (res.get("stderr") or "").lower() or "404" in (res.get("stderr") or "")
            record(GROUP, "delete (cleanup)", "n/a", res, status="PASS" if ok else None,
                   notes="" if res["exit_code"] == 0 else "tolerated: resource already gone")
        for p in (create_path, update_path):
            if p and os.path.exists(p):
                try:
                    os.remove(p)
                except OSError:
                    pass

    print(f"[{GROUP}] done")


if __name__ == "__main__":
    run()
