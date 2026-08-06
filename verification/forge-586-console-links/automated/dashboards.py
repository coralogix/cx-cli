"""
Deterministic replay of the `cx dashboards` console-link verification (PR #176).

Every subcommand here had a PASS entry in the original manual run
(OLD_DIR/results/dashboards.jsonl) -- there were no FAILs or SKIPPEDs for this
group, so everything is safe to mechanically replay. No LLM judgment required.

Creates: 1 dashboard + 1 dashboard folder, both with a fresh unique-suffixed
name/label per run, and both are deleted at the end (tolerating "already gone").
"""

import json
import os
import sys
import time
import uuid

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PAYLOADS_DIR = os.path.join(BASE_DIR, "payloads")
GROUP = "dashboards"
FORMATS = ["text", "json", "agents"]


def _run_id():
    return uuid.uuid4().hex[:8]


def _load(name):
    with open(os.path.join(PAYLOADS_DIR, name)) as f:
        return json.load(f)


def _render(name, data):
    """Write a mutated copy of a template payload next to the originals and return its path."""
    path = os.path.join(PAYLOADS_DIR, f"_rendered_{name}")
    with open(path, "w") as f:
        json.dump(data, f)
    return path


def _extract_id(result, label="ID"):
    """Best-effort pull of a freshly-created object's id out of stdout (JSON) or stderr text."""
    stdout = (result.get("stdout") or "").strip()
    if stdout:
        try:
            obj = json.loads(stdout)
            if isinstance(obj, dict) and obj.get("id"):
                return obj["id"]
            if isinstance(obj, list) and obj and isinstance(obj[0], dict) and obj[0].get("id"):
                return obj[0]["id"]
        except (json.JSONDecodeError, ValueError):
            pass
    stderr = result.get("stderr") or ""
    marker = f"{label}: "
    if marker in stderr:
        tail = stderr.split(marker, 1)[1]
        return tail.split(")")[0].strip()
    return None


def run():
    run_id = _run_id()
    dashboard_id = None
    folder_id = None

    try:
        # --- dashboards create (setup) ---
        create_body = _load("dashboards_dashboard_create.json")
        create_body["name"] = f"cx-cli-pr176-automated-{run_id}"
        create_path = _render("dashboards_dashboard_create.json", create_body)
        r = run_cx(
            ["dashboards", "create", "--from-file", create_path],
            extra_flags=["--yes"],
            output_format="json",
        )
        record(GROUP, "create (setup)", "n/a", r)
        dashboard_id = _extract_id(r)
        if not dashboard_id:
            print("dashboards: could not determine created dashboard id, aborting cleanup-dependent steps")
            return
        print("dashboards create ->", dashboard_id)

        # --- dashboards get (3 formats) ---
        for fmt in FORMATS:
            r = run_cx(["dashboards", "get", dashboard_id], output_format=fmt)
            record(GROUP, "get", fmt, r)

        # --- dashboards check (3 formats) ---
        for fmt in FORMATS:
            r = run_cx(["dashboards", "check", dashboard_id], output_format=fmt)
            record(GROUP, "check (stored id)", fmt, r)

        # --- dashboards replace (3 formats) ---
        replace_body = _load("dashboards_dashboard_replace.json")
        replace_body["id"] = dashboard_id
        replace_body["name"] = f"cx-cli-pr176-automated-{run_id} (replaced)"
        replace_path = _render("dashboards_dashboard_replace.json", replace_body)
        for fmt in FORMATS:
            r = run_cx(
                ["dashboards", "replace", "--from-file", replace_path],
                extra_flags=["--yes"],
                output_format=fmt,
            )
            record(GROUP, "replace", fmt, r)

        # --- dashboards catalog (3 formats, read-only, independent of created object) ---
        for fmt in FORMATS:
            r = run_cx(["dashboards", "catalog"], output_format=fmt)
            record(GROUP, "catalog", fmt, r)

        # --- dashboards search (3 formats, read-only, generic query) ---
        # <DESCRIPTION> is a single positional argument -- passing multi-word
        # phrases as separate argv tokens (a prior version of this script did)
        # is a clap usage error (exit 2), not an API call at all.
        for fmt in FORMATS:
            r = run_cx(["dashboards", "search", "smoke test dashboard"], output_format=fmt)
            record(GROUP, "search", fmt, r)

        # --- dashboards query-search --description (3 formats) ---
        for fmt in FORMATS:
            r = run_cx(
                ["dashboards", "query-search", "--description", "error rate"],
                output_format=fmt,
            )
            record(GROUP, "query-search --description", fmt, r)

        # --- dashboards query-search --field (3 formats) ---
        for fmt in FORMATS:
            r = run_cx(
                ["dashboards", "query-search", "--field", "service_name"],
                output_format=fmt,
            )
            record(GROUP, "query-search --field", fmt, r)

        # --- dashboards folders create (setup) ---
        folder_name = f"cx-cli-pr176-automated-folder-{run_id}"
        r = run_cx(
            ["dashboards", "folders", "create", "--name", folder_name],
            extra_flags=["--yes"],
            output_format="json",
        )
        record(GROUP, "folders create (setup)", "n/a", r)
        folder_id = _extract_id(r)
        print("dashboards folders create ->", folder_id)

        # --- dashboards folders list (3 formats) ---
        for fmt in FORMATS:
            r = run_cx(["dashboards", "folders", "list"], output_format=fmt)
            record(GROUP, "folders list", fmt, r)

    finally:
        # --- cleanup: folder then dashboard, tolerating "already gone" ---
        if folder_id:
            r = run_cx(
                ["dashboards", "folders", "delete", folder_id],
                extra_flags=["--yes"],
                output_format="json",
            )
            record(GROUP, "folders delete (cleanup)", "n/a", r)
            if r.get("exit_code") != 0:
                print("dashboards folders cleanup: non-zero exit (tolerated):", r.get("stderr", "")[:200])

        if dashboard_id:
            r = run_cx(
                ["dashboards", "delete", dashboard_id],
                extra_flags=["--yes"],
                output_format="json",
            )
            record(GROUP, "delete (cleanup)", "n/a", r)
            if r.get("exit_code") != 0:
                print("dashboards cleanup: non-zero exit (tolerated):", r.get("stderr", "")[:200])


if __name__ == "__main__":
    run()
