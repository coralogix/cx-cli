"""
Deterministic replay of the `cx webhooks` console-link verification (PR #176).

`webhooks create`'s original run needed one schema-discovery fix (the first
payload had a bad top-level `type` field; the working shape nests everything
under `data`, see payloads/webhooks_webhook_create.json) -- that failed
attempt is not replayed, only the final working shape is used.

Known bug replayed on purpose (safe, read-only-equivalent, pinned cause):
`webhooks create` itself returned `[]`/empty stdout instead of the created
object (confirmed via a follow-up `list`), so this script -- like the
original session -- falls back to `webhooks list` + name-match to discover
the id of what it just created. That bug is fixed on this branch (FORGE-696);
the fallback is kept because it also covers the case where the create
response carries no id at all, and it costs one extra read.

Known 501s replayed on purpose (deterministic, no actual mutation/send ever
happens because the backend rejects the call before doing anything):
`webhooks update <id> --from-file ...` and `webhooks test <id> --yes` both
return `501 Unimplemented` against this backend. Replaying them is a
regression check -- if either ever returns something other than 501, that's
a signal worth a human look.

NOT covered here (see manual/webhooks.md):
  - `webhooks test` against the pre-existing production Slack webhook
    (a real, non-reproducible account resource) -- excluded on purpose.
  - `actions create/get/update/delete/batch/reorder` -- no known-working
    `sourceType` enum value was ever found for `actions create`.

Creates: 1 webhook with a fresh unique name per run, deleted at the end
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
GROUP = "webhooks"
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
    webhook_id = None
    webhook_name = f"cx-cli-pr176-automated-webhook-{run_id}"

    try:
        # --- webhooks types (3 formats, independent) ---
        for fmt in FORMATS:
            r = run_cx(["webhooks", "types"], output_format=fmt)
            record(GROUP, "webhooks types", fmt, r)

        # --- webhooks list (baseline, 3 formats) ---
        for fmt in FORMATS:
            r = run_cx(["webhooks", "list"], output_format=fmt)
            record(GROUP, "webhooks list", fmt, r)

        # --- webhooks create (setup); known bug: stdout comes back empty/[] ---
        create_body = _load("webhooks_webhook_create.json")
        create_body["data"]["name"] = webhook_name
        create_path = _render("webhooks_webhook_create.json", create_body)
        r = run_cx(
            ["webhooks", "create", "--from-file", create_path],
            extra_flags=["--yes"],
            output_format="json",
        )
        record(GROUP, "webhooks create (setup)", "n/a", r)
        webhook_id = _find_id_by_name(["webhooks", "list"], webhook_name)
        if not webhook_id:
            print("webhooks: could not determine created webhook id, aborting")
            return
        print("webhooks create ->", webhook_id)

        # --- webhooks get (3 formats) ---
        for fmt in FORMATS:
            r = run_cx(["webhooks", "get", webhook_id], output_format=fmt)
            record(GROUP, "webhooks get", fmt, r)

        # --- webhooks update (3 formats); known 501 Unimplemented ---
        update_body = _load("webhooks_webhook_update.json")
        update_body["data"]["id"] = webhook_id
        update_body["data"]["name"] = f"{webhook_name}-updated"
        update_path = _render("webhooks_webhook_update.json", update_body)
        for fmt in FORMATS:
            r = run_cx(
                ["webhooks", "update", webhook_id, "--from-file", update_path],
                extra_flags=["--yes"],
                output_format=fmt,
            )
            record(GROUP, "webhooks update", fmt, r)

        # --- webhooks test (3 formats); known 501 Unimplemented, no real send happens ---
        for fmt in FORMATS:
            r = run_cx(
                ["webhooks", "test", webhook_id],
                extra_flags=["--yes"],
                output_format=fmt,
            )
            record(GROUP, "webhooks test", fmt, r)

        # --- actions list (3 formats, read-only, independent) ---
        for fmt in FORMATS:
            r = run_cx(["webhooks", "actions", "list"], output_format=fmt)
            record(GROUP, "actions list", fmt, r)

    finally:
        if webhook_id:
            r = run_cx(
                ["webhooks", "delete", webhook_id],
                extra_flags=["--yes"],
                output_format="json",
            )
            record(GROUP, "webhooks delete (cleanup)", "n/a", r)
            if r.get("exit_code") != 0:
                print("webhooks cleanup: non-zero exit (tolerated):", r.get("stderr", "")[:200])


if __name__ == "__main__":
    run()
