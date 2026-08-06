"""
Automated (deterministic, no-LLM) replay of the console-link verification for `cx tco`.

Source of truth: OLD_DIR/results/tco.jsonl + OLD_DIR/run_tco_1_create.py through
run_tco_4_cleanup.py.

Covers the confirmed-clean single-policy lifecycle (id 4d67628b-... in the old run - the
one create/get/list/settings/update/test/delete sequence that has a fully recorded,
successful create and a fully recorded, successful delete):

  create -> get[text/json/agents] -> list[text/json/agents] -> settings[text/json/agents]
  -> update (rename) -> test[text/json/agents] -> delete (cleanup)

NOT covered here (see ../manual/tco.md):
  - `reorder`: PASSed last time with a known-working payload shape
    ({"sourceType": <int enum>, "orders": [{"id","order"}]}), but reordering re-numbers
    the priority order of ALL TCO policies of that sourceType for the live team, not just
    the throwaway one - a real (if brief) production side effect that needs a judgment
    call each time, not a mechanical replay.
  - `settings-update`: known, stable 501 Unimplemented baseline for this team/plan (same
    backend limitation as `retentions update`) - re-running a mutating call we already
    know will 501 is not a useful automated check; only worth revisiting if the code path
    changes.
  - The old run also left behind two extra, unexplained policy ids (5d17efb2-...,
    a79e70e8-...) from unrecorded manual discovery probing ("auto-created sibling"
    theory, never confirmed) - both were already gone (404) by cleanup time, so there is
    nothing live to reconcile, but the create path that produced them is not reproduced
    here since it was never a clean, understood, single-call shape.

Each run creates a TCO policy with a fresh unique name suffix so repeated runs never
collide, and deletes it again at the end regardless of outcome (tolerating "already gone").
"""

import json
import os
import sys
import uuid

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record

GROUP = "tco"
HERE = os.path.dirname(os.path.abspath(__file__))
BASE_DIR = os.path.dirname(HERE)
PAYLOAD_DIR = os.path.join(BASE_DIR, "payloads")
CREATE_TEMPLATE = os.path.join(PAYLOAD_DIR, "tco_create.json")
TEST_PAYLOAD = os.path.join(PAYLOAD_DIR, "tco_test.json")


def _write_temp(body, name):
    path = os.path.join(PAYLOAD_DIR, name)
    with open(path, "w") as f:
        json.dump(body, f)
    return path


def run():
    suffix = uuid.uuid4().hex[:8]

    with open(CREATE_TEMPLATE) as f:
        create_body = json.load(f)
    create_body["name"] = f"pr176-tco-auto-test-{suffix}"
    create_path = _write_temp(create_body, f"tco_create_{suffix}.json")

    policy_id = None
    update_path = None
    try:
        res = run_cx(["tco", "create", "--from-file", create_path], output_format="json", extra_flags=["--yes"])
        record(GROUP, "create (setup)", "n/a", res)
        if res["exit_code"] == 0:
            try:
                policy_id = json.loads(res["stdout"]).get("id")
            except (json.JSONDecodeError, AttributeError):
                pass

        if not policy_id:
            print(f"[{GROUP}] ABORT: no id captured from create; skipping rest of lifecycle")
            return

        for fmt in ["text", "json", "agents"]:
            res = run_cx(["tco", "get", policy_id], output_format=fmt)
            record(GROUP, "get", fmt, res)

        for fmt in ["text", "json", "agents"]:
            res = run_cx(["tco", "list"], output_format=fmt)
            record(GROUP, "list", fmt, res)

        for fmt in ["text", "json", "agents"]:
            res = run_cx(["tco", "settings"], output_format=fmt)
            record(GROUP, "settings", fmt, res)

        update_body = dict(create_body)
        update_body["id"] = policy_id
        update_body["name"] = f"pr176-tco-auto-test-{suffix}-updated"
        update_path = _write_temp(update_body, f"tco_update_{suffix}.json")
        res = run_cx(["tco", "update", "--from-file", update_path], output_format="json", extra_flags=["--yes"])
        record(GROUP, "update", "n/a", res)

        for fmt in ["text", "json", "agents"]:
            res = run_cx(["tco", "test", "--from-file", TEST_PAYLOAD], output_format=fmt)
            record(GROUP, "test", fmt, res, notes="ran with `{}` body file; backend accepts and returns empty testPoliciesBulkResult")
    finally:
        if policy_id:
            res = run_cx(["tco", "delete", policy_id], output_format="json", extra_flags=["--yes"])
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
