"""
Deterministic replay of the `cx views` console-link verification (PR #176).

Known bug replayed on purpose (pinned cause, safe to replay): `views create`
prints nothing in text mode and an empty array in json/agents mode instead of
the created view object, in every output format -- confirmed in the original
run by cross-checking a follow-up `views list`. This script does the same:
create, then resolve the new id via `views list` + name-match. Expect
`create` to keep showing as a FAIL for the missing consoleUrl (see
manual/views.md) unless that's been fixed upstream since 2026-08-03.

One throwaway view *and* one throwaway view folder are created and driven
through get/update/delete per output format, using the exact payload shapes
from the original run (payloads/views_view_create_*.json,
views_view_update_*.json, views_folder_create_*.json,
views_folder_update_*.json) with a fresh unique name/id injected per run.

`views folders update` is a known, deterministic 501 Unimplemented -- also
replayed on purpose (mutation never actually applies, so it's side-effect
free).

Everything created here is deleted at the end, tolerating "already gone".
"""

import json
import os
import sys
import uuid

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PAYLOADS_DIR = os.path.join(BASE_DIR, "payloads")
GROUP = "views"
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


def _extract_id_from_stdout(result):
    stdout = (result.get("stdout") or "").strip()
    if not stdout:
        return None
    try:
        obj = json.loads(stdout)
    except (json.JSONDecodeError, ValueError):
        return None
    if isinstance(obj, dict) and obj.get("id"):
        return obj["id"]
    if isinstance(obj, list) and obj and isinstance(obj[0], dict) and obj[0].get("id"):
        return obj[0]["id"]
    return None


def _find_id_by_name(list_args, name_value, name_field="name"):
    r = run_cx(list_args, output_format="json")
    stdout = (r.get("stdout") or "").strip()
    try:
        items = json.loads(stdout)
    except (json.JSONDecodeError, ValueError):
        return None
    if isinstance(items, dict):
        for v in items.values():
            if isinstance(v, list):
                items = v
                break
    if not isinstance(items, list):
        return None
    for item in items:
        if isinstance(item, dict) and item.get(name_field) == name_value:
            return item.get("id")
    return None


def run():
    run_id = _run_id()
    view_ids = {}  # fmt -> id, for final safety-net cleanup
    folder_ids = {}  # fmt -> id, for final safety-net cleanup

    try:
        # ================= views: create -> get -> update -> delete, per format =================
        for fmt in FORMATS:
            view_name = f"cx-cli-pr176-automated-view-{fmt}-{run_id}"
            create_body = _load(f"views_view_create_{fmt}.json")
            create_body["name"] = view_name
            create_path = _render(f"views_view_create_{fmt}.json", create_body)

            r = run_cx(
                ["views", "create", "--from-file", create_path],
                extra_flags=["--yes"],
                output_format=fmt,
            )
            record(GROUP, "create", fmt, r)

            view_id = _extract_id_from_stdout(r) or _find_id_by_name(["views", "list"], view_name)
            if not view_id:
                print(f"views: could not determine created view id for fmt={fmt}, skipping")
                continue
            view_ids[fmt] = view_id
            print(f"views create ({fmt}) ->", view_id)

            r = run_cx(["views", "get", str(view_id)], output_format=fmt)
            record(GROUP, "get", fmt, r)

            update_body = _load(f"views_view_update_{fmt}.json")
            update_body["id"] = view_id
            update_body["name"] = f"{view_name}-UPDATED"
            update_path = _render(f"views_view_update_{fmt}.json", update_body)
            r = run_cx(
                ["views", "update", str(view_id), "--from-file", update_path],
                extra_flags=["--yes"],
                output_format=fmt,
            )
            record(GROUP, "update", fmt, r)

            r = run_cx(
                ["views", "delete", str(view_id)],
                extra_flags=["--yes"],
                output_format=fmt,
            )
            record(GROUP, "delete", fmt, r)
            if r.get("exit_code") == 0:
                del view_ids[fmt]  # already cleaned up inline

        # ================= views folders: create -> list -> get -> update -> delete =================
        for fmt in FORMATS:
            folder_name = f"cx-cli-pr176-automated-view-folder-{fmt}-{run_id}"
            create_body = _load(f"views_folder_create_{fmt}.json")
            create_body["name"] = folder_name
            create_path = _render(f"views_folder_create_{fmt}.json", create_body)

            r = run_cx(
                ["views", "folders", "create", "--from-file", create_path],
                extra_flags=["--yes"],
                output_format=fmt,
            )
            record(GROUP, "folders create (setup)", fmt, r)

            folder_id = _extract_id_from_stdout(r) or _find_id_by_name(
                ["views", "folders", "list"], folder_name
            )
            if not folder_id:
                print(f"views: could not determine created folder id for fmt={fmt}, skipping")
                continue
            folder_ids[fmt] = folder_id
            print(f"views folders create ({fmt}) ->", folder_id)

        for fmt in FORMATS:
            r = run_cx(["views", "folders", "list"], output_format=fmt)
            record(GROUP, "folders list", fmt, r)

        for fmt in FORMATS:
            folder_id = folder_ids.get(fmt)
            if not folder_id:
                continue
            r = run_cx(["views", "folders", "get", folder_id], output_format=fmt)
            record(GROUP, "folders get", fmt, r)

        for fmt in FORMATS:
            folder_id = folder_ids.get(fmt)
            if not folder_id:
                continue
            update_body = _load(f"views_folder_update_{fmt}.json")
            update_body["id"] = folder_id
            update_body["name"] = f"cx-cli-pr176-automated-view-folder-{fmt}-{run_id}-UPDATED"
            update_path = _render(f"views_folder_update_{fmt}.json", update_body)
            r = run_cx(
                ["views", "folders", "update", folder_id, "--from-file", update_path],
                extra_flags=["--yes"],
                output_format=fmt,
            )
            record(GROUP, "folders update", fmt, r)

        for fmt in FORMATS:
            folder_id = folder_ids.get(fmt)
            if not folder_id:
                continue
            r = run_cx(
                ["views", "folders", "delete", folder_id],
                extra_flags=["--yes"],
                output_format=fmt,
            )
            record(GROUP, "folders delete", fmt, r)
            if r.get("exit_code") == 0:
                del folder_ids[fmt]  # already cleaned up inline

    finally:
        # --- safety net: delete anything not already cleaned up above, tolerating "already gone" ---
        for fmt, view_id in list(view_ids.items()):
            r = run_cx(["views", "delete", str(view_id)], extra_flags=["--yes"], output_format="json")
            record(GROUP, "delete (cleanup)", "n/a", r)
            if r.get("exit_code") != 0:
                print(f"views cleanup ({fmt}) non-zero exit (tolerated):", r.get("stderr", "")[:200])

        for fmt, folder_id in list(folder_ids.items()):
            r = run_cx(
                ["views", "folders", "delete", folder_id],
                extra_flags=["--yes"],
                output_format="json",
            )
            record(GROUP, "folders delete (cleanup)", "n/a", r)
            if r.get("exit_code") != 0:
                print(
                    f"views folder cleanup ({fmt}) non-zero exit (tolerated):",
                    r.get("stderr", "")[:200],
                )


if __name__ == "__main__":
    run()
