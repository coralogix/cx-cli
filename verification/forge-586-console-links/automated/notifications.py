"""
Deterministic replay of the `cx notifications` console-link verification (PR #176).

Covers: connectors (types/list/create/get/update/entity-types/entity-subtypes/
delete), routers (create/get/update/list/validate-matcher/delete), presets
(create/get/list/update/set-default/delete), and test (connector/destination/
template-render). All use the FINAL known-working payload shapes from the
original run -- the schema-discovery FAILs that preceded them (bad top-level
`type` field on connector create, empty `routingLabels` on router create,
missing `connector_type`/`entity_type`/`configOverrides` on preset create)
are historical noise and are NOT replayed.

`connectors entity-types` / `entity-subtypes --type SLACK` are known,
deterministic 404s (the CLI/API appears to route these as a
"get connector by id" lookup rather than a real endpoint) -- replaying them
is safe (read-only, no side effects) and doubles as a regression check: if
they ever start returning 200, that's a signal the underlying bug got fixed.

NOT covered here (see manual/notifications.md):
  - `test preset`: no known-working request body was ever found (schema
    discovery abandoned after multiple 400s). `test routing-condition` used
    to be in this bucket too, but its schema was cracked (see below) and it's
    now part of the automated replay above.
  - the supplemental `webhooks test` against a pre-existing production Slack
    connector -- not applicable to this group, see webhooks.

Creates: 1 connector, 1 router, 1 preset -- each with a fresh unique
name/label per run -- all deleted at the end (router/preset/connector, in
that dependency order), tolerating "already gone". `presets set-default` is
restored back to the system default preset afterward.
"""

import json
import os
import sys
import uuid

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PAYLOADS_DIR = os.path.join(BASE_DIR, "payloads")
GROUP = "notifications"
FORMATS = ["text", "json", "agents"]
SYSTEM_DEFAULT_PRESET = "preset_system_slack_alerts_basic"


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
    if isinstance(obj, dict):
        if obj.get("id"):
            return obj["id"]
        for key in ("connector", "router", "preset"):
            nested = obj.get(key)
            if isinstance(nested, dict) and nested.get("id"):
                return nested["id"]
    return None


def _find_id_by_name(list_args, name_value, name_field="name"):
    r = run_cx(list_args, output_format="json")
    stdout = (r.get("stdout") or "").strip()
    try:
        items = json.loads(stdout)
    except (json.JSONDecodeError, ValueError):
        return None
    if isinstance(items, dict):
        # some list endpoints wrap the array, e.g. {"connectors": [...]}
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
    connector_id = None
    router_id = None
    preset_id = None
    connector_name = f"cx-cli-pr176-automated-connector-{run_id}"
    router_name = f"cx-cli-pr176-automated-router-{run_id}"
    preset_name = f"cx-cli-pr176-automated-preset-{run_id}"

    try:
        # --- connectors types (3 formats) ---
        for fmt in FORMATS:
            r = run_cx(["notifications", "connectors", "types"], output_format=fmt)
            record(GROUP, "connectors types", fmt, r)

        # --- connectors list (baseline, 3 formats) ---
        for fmt in FORMATS:
            r = run_cx(["notifications", "connectors", "list"], output_format=fmt)
            record(GROUP, "connectors list", fmt, r)

        # --- connectors create (setup), known-working nested shape ---
        create_body = _load("notifications_connector_create.json")
        create_body["connector"]["name"] = connector_name
        create_path = _render("notifications_connector_create.json", create_body)
        r = run_cx(
            ["notifications", "connectors", "create", "--from-file", create_path],
            extra_flags=["--yes"],
            output_format="json",
        )
        record(GROUP, "connectors create (setup)", "n/a", r)
        connector_id = _extract_id_from_stdout(r) or _find_id_by_name(
            ["notifications", "connectors", "list"], connector_name
        )
        if not connector_id:
            print("notifications: could not determine created connector id, aborting")
            return
        print("connectors create ->", connector_id)

        # --- connectors get (3 formats) ---
        for fmt in FORMATS:
            r = run_cx(["notifications", "connectors", "get", connector_id], output_format=fmt)
            record(GROUP, "connectors get", fmt, r)

        # --- connectors update (3 formats) ---
        update_body = _load("notifications_connector_update.json")
        update_body["connector"]["id"] = connector_id
        update_body["connector"]["name"] = connector_name
        update_body["connector"]["description"] = (
            "PR176 automated verification connector (updated)"
        )
        update_path = _render("notifications_connector_update.json", update_body)
        for fmt in FORMATS:
            r = run_cx(
                ["notifications", "connectors", "update", "--from-file", update_path],
                extra_flags=["--yes"],
                output_format=fmt,
            )
            record(GROUP, "connectors update", fmt, r)

        # --- connectors entity-types / entity-subtypes (known 404s, 3 formats each) ---
        for fmt in FORMATS:
            r = run_cx(["notifications", "connectors", "entity-types"], output_format=fmt)
            record(GROUP, "connectors entity-types", fmt, r)
        for fmt in FORMATS:
            r = run_cx(
                ["notifications", "connectors", "entity-subtypes", "--type", "SLACK"],
                output_format=fmt,
            )
            record(GROUP, "connectors entity-subtypes", fmt, r)

        # --- routers create (setup), needs non-empty routingLabels ---
        router_body = _load("notifications_router_create.json")
        router_body["router"]["name"] = router_name
        router_body["router"]["rules"][0]["targets"][0]["connectorId"] = connector_id
        router_path = _render("notifications_router_create.json", router_body)
        r = run_cx(
            ["notifications", "routers", "create", "--from-file", router_path],
            extra_flags=["--yes"],
            output_format="json",
        )
        record(GROUP, "routers create (setup)", "n/a", r)
        router_id = _extract_id_from_stdout(r) or _find_id_by_name(
            ["notifications", "routers", "list"], router_name
        )
        if not router_id:
            print("notifications: could not determine created router id, skipping router steps")
        else:
            print("routers create ->", router_id)

            # --- routers get (3 formats) ---
            for fmt in FORMATS:
                r = run_cx(["notifications", "routers", "get", router_id], output_format=fmt)
                record(GROUP, "routers get", fmt, r)

            # --- routers update (3 formats) ---
            router_update_body = _load("notifications_router_update.json")
            router_update_body["router"]["id"] = router_id
            router_update_body["router"]["name"] = router_name
            router_update_body["router"]["description"] = (
                "PR176 automated verification router (updated)"
            )
            router_update_body["router"]["rules"][0]["targets"][0]["connectorId"] = connector_id
            router_update_path = _render(
                "notifications_router_update.json", router_update_body
            )
            for fmt in FORMATS:
                r = run_cx(
                    ["notifications", "routers", "update", "--from-file", router_update_path],
                    extra_flags=["--yes"],
                    output_format=fmt,
                )
                record(GROUP, "routers update", fmt, r)

            # --- routers list (3 formats) ---
            for fmt in FORMATS:
                r = run_cx(["notifications", "routers", "list"], output_format=fmt)
                record(GROUP, "routers list", fmt, r)

            # --- routers validate-matcher (3 formats, static payload) ---
            matcher_path = os.path.join(PAYLOADS_DIR, "notifications_validate_matcher.json")
            for fmt in FORMATS:
                r = run_cx(
                    ["notifications", "routers", "validate-matcher", "--from-file", matcher_path],
                    output_format=fmt,
                )
                record(GROUP, "routers validate-matcher", fmt, r)

        # --- presets create (setup), needs connector_type/entity_type/configOverrides ---
        preset_body = _load("notifications_preset_create.json")
        preset_body["preset"]["name"] = preset_name
        preset_path = _render("notifications_preset_create.json", preset_body)
        r = run_cx(
            ["notifications", "presets", "create", "--from-file", preset_path],
            extra_flags=["--yes"],
            output_format="json",
        )
        record(GROUP, "presets create (setup)", "n/a", r)
        preset_id = _extract_id_from_stdout(r) or _find_id_by_name(
            ["notifications", "presets", "list"], preset_name
        )
        if not preset_id:
            print("notifications: could not determine created preset id, skipping preset steps")
        else:
            print("presets create ->", preset_id)

            # --- presets get (3 formats) ---
            for fmt in FORMATS:
                r = run_cx(["notifications", "presets", "get", preset_id], output_format=fmt)
                record(GROUP, "presets get", fmt, r)

            # --- presets list (3 formats) ---
            for fmt in FORMATS:
                r = run_cx(["notifications", "presets", "list"], output_format=fmt)
                record(GROUP, "presets list", fmt, r)

            # --- presets update (3 formats) ---
            preset_update_body = _load("notifications_preset_update.json")
            preset_update_body["preset"]["id"] = preset_id
            preset_update_body["preset"]["name"] = preset_name
            preset_update_body["preset"]["description"] = (
                "PR176 automated verification preset (updated)"
            )
            preset_update_path = _render(
                "notifications_preset_update.json", preset_update_body
            )
            for fmt in FORMATS:
                r = run_cx(
                    ["notifications", "presets", "update", "--from-file", preset_update_path],
                    extra_flags=["--yes"],
                    output_format=fmt,
                )
                record(GROUP, "presets update", fmt, r)

            # --- presets set-default (3 formats), then restore system default ---
            try:
                for fmt in FORMATS:
                    r = run_cx(
                        ["notifications", "presets", "set-default", preset_id],
                        extra_flags=["--yes"],
                        output_format=fmt,
                    )
                    record(GROUP, "presets set-default", fmt, r)
            finally:
                r = run_cx(
                    ["notifications", "presets", "set-default", SYSTEM_DEFAULT_PRESET],
                    extra_flags=["--yes"],
                    output_format="json",
                )
                record(GROUP, "presets set-default restore (cleanup)", "n/a", r)
                if r.get("exit_code") != 0:
                    print(
                        "notifications: FAILED to restore system default preset -- "
                        "manual intervention required:",
                        r.get("stderr", "")[:300],
                    )

        # --- test connector / destination / template-render (3 formats each) ---
        test_connector_path = os.path.join(PAYLOADS_DIR, "notifications_test_connector.json")
        for fmt in FORMATS:
            r = run_cx(
                ["notifications", "test", "connector", "--from-file", test_connector_path],
                output_format=fmt,
            )
            record(GROUP, "test connector", fmt, r)

        test_dest_body = _load("notifications_test_destination.json")
        test_dest_body["connectorId"] = connector_id
        test_dest_path = _render("notifications_test_destination.json", test_dest_body)
        for fmt in FORMATS:
            r = run_cx(
                ["notifications", "test", "destination", "--from-file", test_dest_path],
                output_format=fmt,
            )
            record(GROUP, "test destination", fmt, r)

        test_template_path = os.path.join(
            PAYLOADS_DIR, "notifications_test_template_render.json"
        )
        for fmt in FORMATS:
            r = run_cx(
                ["notifications", "test", "template-render", "--from-file", test_template_path],
                output_format=fmt,
            )
            record(GROUP, "test template-render", fmt, r)

        # test routing-condition: the required field is `template` (same name as
        # test template-render's own body, despite being a distinct subcommand).
        # The original session's 6 guesses (condition/expression/matcher/
        # routingCondition/entityMatcher/conditionExpression) were all wrong;
        # `template` was never tried until a 2026-08-06 manual re-verification
        # pass found it via the bare-entityType error message ("template must
        # not be empty"). Promoted here from manual/notifications.md.
        test_routing_path = os.path.join(
            PAYLOADS_DIR, "notifications_test_routing_condition.json"
        )
        for fmt in FORMATS:
            r = run_cx(
                ["notifications", "test", "routing-condition", "--from-file", test_routing_path],
                output_format=fmt,
            )
            record(GROUP, "test routing-condition", fmt, r)

    finally:
        # --- cleanup: router, preset, connector, tolerating "already gone" ---
        if router_id:
            r = run_cx(
                ["notifications", "routers", "delete", router_id],
                extra_flags=["--yes"],
                output_format="json",
            )
            record(GROUP, "routers delete (cleanup)", "n/a", r)
            if r.get("exit_code") != 0:
                print("notifications router cleanup (tolerated):", r.get("stderr", "")[:200])

        if preset_id:
            r = run_cx(
                ["notifications", "presets", "delete", preset_id],
                extra_flags=["--yes"],
                output_format="json",
            )
            record(GROUP, "presets delete (cleanup)", "n/a", r)
            if r.get("exit_code") != 0:
                print("notifications preset cleanup (tolerated):", r.get("stderr", "")[:200])

        if connector_id:
            r = run_cx(
                ["notifications", "connectors", "delete", connector_id],
                extra_flags=["--yes"],
                output_format="json",
            )
            record(GROUP, "connectors delete (cleanup)", "n/a", r)
            if r.get("exit_code") != 0:
                print("notifications connector cleanup (tolerated):", r.get("stderr", "")[:200])


if __name__ == "__main__":
    run()
