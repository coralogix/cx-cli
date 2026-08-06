"""
Deterministic replay of the `cx slos` console-link verification (PR #176).

The original run needed several schema-discovery attempts to find a valid
`slos create` body (see OLD_DIR/step6_slos.py / step7_slos.py / step8_slos.py):
top-level `serviceName`/`filters` are invalid, `sli` (via the
`requestBasedMetricSli` oneof) is required, and the PromQL queries inside it
must use a `[1m]` range. Those discovery FAILs are historical noise, not
replayed here -- only the final known-working payload
(payloads/slos_slo_create.json) is used.

`slos update` takes no positional id -- the id must be embedded in the JSON
body (full-replace semantics); passing it positionally is a CLI usage error,
also not replayed here.

Creates: 1 SLO with a fresh unique name per run, deleted at the end
(tolerating "already gone").
"""

import json
import os
import sys
import uuid

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PAYLOADS_DIR = os.path.join(BASE_DIR, "payloads")
GROUP = "slos"
FORMATS = ["text", "json", "agents"]


def _run_id():
    return uuid.uuid4().hex[:8]


def _load(name):
    with open(os.path.join(PAYLOADS_DIR, name)) as f:
        return json.load(f)


def _render(name, data):
    path = os.path.join(PAYLOADS_DIR, f"_rendered_{name}")
    with open(path, "w") as f:
        json.dump(data, f)
    return path


def _extract_id(result):
    stdout = (result.get("stdout") or "").strip()
    if stdout:
        try:
            obj = json.loads(stdout)
            if isinstance(obj, dict) and obj.get("id"):
                return obj["id"]
        except (json.JSONDecodeError, ValueError):
            pass
    stderr = result.get("stderr") or ""
    marker = "ID: "
    if marker in stderr:
        return stderr.split(marker, 1)[1].split(")")[0].strip()
    return None


def run():
    run_id = _run_id()
    slo_id = None

    try:
        # --- slos list (baseline, 3 formats) ---
        for fmt in FORMATS:
            r = run_cx(["slos", "list"], output_format=fmt)
            record(GROUP, "list", fmt, r)

        # --- slos create (setup), known-working requestBasedMetricSli shape ---
        create_body = _load("slos_slo_create.json")
        create_body["name"] = f"cx-cli-pr176-automated-slo-{run_id}"
        create_path = _render("slos_slo_create.json", create_body)
        r = run_cx(
            ["slos", "create", "--from-file", create_path],
            extra_flags=["--yes"],
            output_format="json",
        )
        record(GROUP, "create (setup, success)", "json", r)
        slo_id = _extract_id(r)
        if not slo_id:
            print("slos: could not determine created SLO id, aborting cleanup-dependent steps")
            return
        print("slos create ->", slo_id)

        # --- slos get (3 formats) ---
        for fmt in FORMATS:
            r = run_cx(["slos", "get", slo_id], output_format=fmt)
            record(GROUP, "get", fmt, r)

        # --- slos list (populated, 3 formats) ---
        for fmt in FORMATS:
            r = run_cx(["slos", "list"], output_format=fmt)
            record(GROUP, "list (populated)", fmt, r)

        # --- slos update (3 formats); id lives in the body, full-replace semantics ---
        update_body = _load("slos_slo_update.json")
        update_body["id"] = slo_id
        update_body["name"] = f"cx-cli-pr176-automated-slo-{run_id} (updated)"
        update_path = _render("slos_slo_update.json", update_body)
        for fmt in FORMATS:
            r = run_cx(
                ["slos", "update", "--from-file", update_path],
                extra_flags=["--yes"],
                output_format=fmt,
            )
            record(GROUP, "update", fmt, r)

    finally:
        if slo_id:
            r = run_cx(["slos", "delete", slo_id], extra_flags=["--yes"], output_format="json")
            record(GROUP, "delete (cleanup)", "n/a", r)
            if r.get("exit_code") != 0:
                print("slos cleanup: non-zero exit (tolerated):", r.get("stderr", "")[:200])


if __name__ == "__main__":
    run()
