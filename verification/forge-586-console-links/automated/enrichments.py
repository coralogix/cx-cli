"""
Automated (deterministic, no-LLM) replay of the console-link verification for `cx enrichments`.

Source of truth: OLD_DIR/results/enrichments.jsonl + OLD_DIR/run_enrichments.py,
run_enrichments2.py, run_custom_enrichments.py.

Covers two independent lifecycles:

1. Built-in enrichments read-only surface (list / limit / settings) - PASS last time,
   pure GET calls, no side effects.

2. Custom enrichment tables full lifecycle (create -> get -> update -> list -> delete),
   which worked cleanly end-to-end last time with a confirmed working delete route.

NOT covered here (see ../manual/enrichments.md):
  - `enrichments add` / `enrichments overwrite` / `enrichments remove`: the old run found
    that `add` creates enrichment stubs (fieldName-based, e.g. suspiciousIp) that
    `remove` reports as removed (200 OK) but does NOT actually delete - confirmed via a
    follow-up `list` still showing them. Two such orphaned stubs (ids 244123, 244124)
    were left behind with no working delete route. `overwrite` also FAILs with a 400
    "enrichmentType is required" backend validation bug even with a well-formed body.
    Automating `add` would create more of these permanent orphans on every run.
  - `enrichments custom search`: FAILed with a 404 last time even against a
    freshly-created, confirmed-existing table - a known environment/API limitation.

Each run creates a custom enrichment table with a fresh unique name suffix so repeated
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

GROUP = "enrichments"
HERE = os.path.dirname(os.path.abspath(__file__))
BASE_DIR = os.path.dirname(HERE)
PAYLOAD_DIR = os.path.join(BASE_DIR, "payloads")
TEMPLATE = os.path.join(PAYLOAD_DIR, "enrichments_custom_create.json")

ID_RE = re.compile(r'"(\d+)"')


def _write_temp(body, name):
    path = os.path.join(PAYLOAD_DIR, name)
    with open(path, "w") as f:
        json.dump(body, f, indent=2)
    return path


def _run_builtin_readonly():
    for fmt in ["text", "json", "agents"]:
        res = run_cx(["enrichments", "list"], output_format=fmt)
        record(GROUP, "list", fmt, res)

    for fmt in ["text", "json", "agents"]:
        res = run_cx(["enrichments", "limit"], output_format=fmt)
        record(GROUP, "limit", fmt, res)

    for fmt in ["text", "json", "agents"]:
        res = run_cx(["enrichments", "settings"], output_format=fmt)
        record(GROUP, "settings", fmt, res)


def _run_custom_lifecycle():
    suffix = uuid.uuid4().hex[:8]

    with open(TEMPLATE) as f:
        create_body = json.load(f)
    create_body["name"] = f"cx-cli-pr176-auto-table-{suffix}"
    create_path = _write_temp(create_body, f"enrichments_custom_create_{suffix}.json")

    ce_id = None
    update_path = None
    try:
        res = run_cx(
            ["enrichments", "custom", "create", "--from-file", create_path],
            output_format=None,
            extra_flags=["--yes"],
        )
        record(GROUP, "custom create (setup)", "n/a", res)
        if res["exit_code"] == 0:
            m = ID_RE.search(res["stdout"])
            if m:
                ce_id = m.group(1)

        if not ce_id:
            print(f"[{GROUP}] ABORT: no id captured from custom create; skipping rest of custom lifecycle")
            return

        for fmt in ["text", "json", "agents"]:
            res = run_cx(["enrichments", "custom", "get", ce_id], output_format=fmt)
            record(GROUP, "custom get", fmt, res)

        update_body = dict(create_body)
        update_body["customEnrichmentId"] = int(ce_id)
        update_body["description"] = (
            "Temporary automated test custom enrichment table for PR176 verification (UPDATED) - safe to delete"
        )
        update_path = _write_temp(update_body, f"enrichments_custom_update_{suffix}.json")
        for fmt in ["text", "json", "agents"]:
            res = run_cx(
                ["enrichments", "custom", "update", "--from-file", update_path],
                output_format=fmt,
                extra_flags=["--yes"],
            )
            record(GROUP, "custom update", fmt, res)

        for fmt in ["text", "json", "agents"]:
            res = run_cx(["enrichments", "custom", "list"], output_format=fmt)
            record(GROUP, "custom list", fmt, res)
    finally:
        if ce_id:
            res = run_cx(["enrichments", "custom", "delete", ce_id], output_format=None, extra_flags=["--yes"])
            ok = res["exit_code"] == 0 or "not found" in (res.get("stderr") or "").lower() or "404" in (res.get("stderr") or "")
            record(GROUP, "custom delete (cleanup)", "n/a", res, status="PASS" if ok else None,
                   notes="" if res["exit_code"] == 0 else "tolerated: resource already gone")
        for p in (create_path, update_path):
            if p and os.path.exists(p):
                try:
                    os.remove(p)
                except OSError:
                    pass


def run():
    _run_builtin_readonly()
    _run_custom_lifecycle()
    print(f"[{GROUP}] done")


if __name__ == "__main__":
    run()
