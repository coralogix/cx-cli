"""
Deterministic replay of the `cx iam` console-link verification (PR #176).

`iam` is the biggest and messiest group in the original run (129 JSONL
entries across roles/scopes/groups/api-keys/users/ip-access) because most
subcommands need a real `--from-file` JSON body and the original session
spent a lot of its time on live schema discovery (trial-and-error against
the real API's proto validator) before landing on a working shape. Only the
subcommands that ended in a confirmed-working shape are replayed here; see
manual/iam.md for everything that never got past discovery, needs a real
(non-throwaway) target, or hit an unresolved backend/CLI bug.

Replayed (each creates fresh, uniquely-named-per-run throwaway entities and
cleans them up immediately after, tolerating "already gone" on cleanup):

  - roles:     list, system (read-only) + create -> get -> update -> delete
  - groups:    list (read-only) + create -> get -> get-by-name -> update -> delete
  - scopes:    list (read-only only -- create/update/get/delete never had a
               working payload, see manual/iam.md)
  - api-keys:  send-data-keys (read-only) + create -> get -> update -> delete
               (own throwaway key) + create -> admin delete (admin-ops path)
  - users:     search + get (read-only only -- create/update/set-status all
               either lack a safe throwaway target or never had a working
               payload, see manual/iam.md). `get` takes the numeric
               user_account_id (NOT the UUID user_id from search results) --
               confirmed working against this operator's own real account
               (id 37832), which is a safe, stable, known-good target.
  - ip-access: get (read-only baseline) + create -> update -> delete, but
               ONLY if the live baseline is currently empty (see the safety
               guard in ip_access_flow) -- this is a real, global,
               team-level security resource, not a per-run throwaway target

NOT replayed (see manual/iam.md for why + baseline/decision criteria):
  - scopes create/update/get/delete   (no payload ever passed validation)
  - groups users                      (404 against every group tried, incl.
                                        a real pre-existing one -- unresolved)
  - api-keys list                     (was FAILing pre-fix; likely fixed by
                                        commit cc496b5, needs fresh comparison)
  - api-keys admin list               (subcommand removed entirely by
                                        commit cc496b5 -- no longer exists)
  - api-keys admin set-status         (real, still-present CLI/backend field
                                        name mismatch -- always FAILs)
  - users create/update/set-status    (no safe/known-working target)
"""

import json
import os
import sys
import uuid

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record, record_skip  # noqa: E402

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PAYLOADS_DIR = os.path.join(BASE_DIR, "payloads")
GROUP = "iam"
FORMATS = ["text", "json", "agents"]
YES = ["--yes"]

# This operator's own real user_id on kb-demo -- used as `owner.user_id`
# when creating throwaway API keys (mirrors what a real caller creating a
# key for themselves sends). Taken verbatim from the known-working
# OLD_DIR/payloads/apikey_*.json files.
SELF_USER_ID = "ed0d7044-5b45-4727-a0de-c4f3a4380ed4"


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
    """Every iam create success message includes '(ID: <value>)' in stderr,
    regardless of output format (info/progress lines always go to stderr,
    even for -o json/agents) -- use that instead of parsing entity-specific
    JSON key names (roles use 'id', groups use 'group_id', api keys use
    'key_id', ip-access uses 'id', ...)."""
    stderr = result.get("stderr") or ""
    marker = "(ID: "
    if marker in stderr:
        return stderr.split(marker, 1)[1].split(")")[0].strip()
    return None


def roles_flow(run_id):
    for fmt in FORMATS:
        r = run_cx(["iam", "roles", "list"], output_format=fmt)
        record(GROUP, "roles list", fmt, r)
    for fmt in FORMATS:
        r = run_cx(["iam", "roles", "system"], output_format=fmt)
        record(GROUP, "roles system", fmt, r)

    for fmt in FORMATS:
        role_id = None
        try:
            body = _load("iam_role_create.json")
            body["name"] = f"cx-cli-pr176-automated-role-{fmt}-{run_id}"
            path = _render("iam_role_create.json", body)
            r = run_cx(
                ["iam", "roles", "create", "--from-file", path],
                extra_flags=YES,
                output_format=fmt,
            )
            record(GROUP, "roles create", fmt, r)
            role_id = _extract_id(r)
            if not role_id:
                print(f"iam roles create ({fmt}): no role id extracted, skipping get/update")
                continue

            r = run_cx(["iam", "roles", "get", role_id], output_format=fmt)
            record(GROUP, "roles get", fmt, r)

            update_body = _load("iam_role_update_minimal.json")
            update_body["role_id"] = role_id
            update_path = _render("iam_role_update_minimal.json", update_body)
            r = run_cx(
                ["iam", "roles", "update", role_id, "--from-file", update_path],
                extra_flags=YES,
                output_format=fmt,
            )
            record(GROUP, "roles update", fmt, r)
        finally:
            if role_id:
                r = run_cx(
                    ["iam", "roles", "delete", role_id], extra_flags=YES, output_format=fmt
                )
                record(GROUP, "roles delete (cleanup)", fmt, r)
                if r.get("exit_code") != 0:
                    print(
                        f"iam roles cleanup ({fmt}): non-zero exit (tolerated): "
                        f"{r.get('stderr', '')[:200]}"
                    )


def groups_flow(run_id):
    for fmt in FORMATS:
        r = run_cx(["iam", "groups", "list"], output_format=fmt)
        record(GROUP, "groups list", fmt, r)

    for fmt in FORMATS:
        group_id = None
        try:
            group_name = f"cx-cli-pr176-automated-group-{fmt}-{run_id}"
            body = _load("iam_group_create.json")
            body["name"] = group_name
            path = _render("iam_group_create.json", body)
            r = run_cx(
                ["iam", "groups", "create", "--from-file", path],
                extra_flags=YES,
                output_format=fmt,
            )
            record(GROUP, "groups create", fmt, r)
            group_id = _extract_id(r)
            if not group_id:
                print(f"iam groups create ({fmt}): no group id extracted, skipping get/update")
                continue

            r = run_cx(["iam", "groups", "get", group_id], output_format=fmt)
            record(GROUP, "groups get", fmt, r)

            r = run_cx(["iam", "groups", "get-by-name", group_name], output_format=fmt)
            record(GROUP, "groups get-by-name", fmt, r)

            update_body = _load("iam_group_update.json")
            update_body["name"] = f"{group_name}-updated"
            update_path = _render("iam_group_update.json", update_body)
            r = run_cx(
                ["iam", "groups", "update", group_id, "--from-file", update_path],
                extra_flags=YES,
                output_format=fmt,
            )
            record(GROUP, "groups update", fmt, r)
        finally:
            if group_id:
                r = run_cx(
                    ["iam", "groups", "delete", group_id], extra_flags=YES, output_format=fmt
                )
                record(GROUP, "groups delete (cleanup)", fmt, r)
                if r.get("exit_code") != 0:
                    print(
                        f"iam groups cleanup ({fmt}): non-zero exit (tolerated): "
                        f"{r.get('stderr', '')[:200]}"
                    )


def scopes_readonly():
    for fmt in FORMATS:
        r = run_cx(["iam", "scopes", "list"], output_format=fmt)
        record(GROUP, "scopes list", fmt, r)


def api_keys_flow(run_id):
    for fmt in FORMATS:
        r = run_cx(["iam", "api-keys", "send-data-keys"], output_format=fmt)
        record(GROUP, "api-keys send-data-keys", fmt, r)

    for fmt in FORMATS:
        key_id = None
        try:
            body = _load("iam_apikey_create.json")
            body["name"] = f"cx-cli-pr176-automated-key-{fmt}-{run_id}"
            path = _render("iam_apikey_create.json", body)
            r = run_cx(
                ["iam", "api-keys", "create", "--from-file", path],
                extra_flags=YES,
                output_format=fmt,
            )
            record(GROUP, "api-keys create", fmt, r)
            key_id = _extract_id(r)
            if not key_id:
                print(f"iam api-keys create ({fmt}): no key id extracted, skipping get/update")
                continue

            r = run_cx(["iam", "api-keys", "get", key_id], output_format=fmt)
            record(GROUP, "api-keys get", fmt, r)

            empty_payload = os.path.join(PAYLOADS_DIR, "iam_apikey_update_empty.json")
            r = run_cx(
                ["iam", "api-keys", "update", key_id, "--from-file", empty_payload],
                extra_flags=YES,
                output_format=fmt,
            )
            record(GROUP, "api-keys update", fmt, r)
        finally:
            if key_id:
                r = run_cx(
                    ["iam", "api-keys", "delete", key_id], extra_flags=YES, output_format=fmt
                )
                record(GROUP, "api-keys delete (cleanup)", fmt, r)
                if r.get("exit_code") != 0:
                    print(
                        f"iam api-keys cleanup ({fmt}): non-zero exit (tolerated): "
                        f"{r.get('stderr', '')[:200]}"
                    )


def api_keys_admin_flow(run_id):
    """create (setup) -> admin delete, one throwaway key per format.
    `admin set-status` is deliberately excluded -- see manual/iam.md, it
    has never once succeeded (real, still-present field-name mismatch bug)."""
    for fmt in FORMATS:
        key_id = None
        try:
            body = _load("iam_apikey_create.json")
            body["name"] = f"cx-cli-pr176-automated-adminkey-{fmt}-{run_id}"
            path = _render("iam_apikey_adminops.json", body)
            r = run_cx(
                ["iam", "api-keys", "create", "--from-file", path],
                extra_flags=YES,
                output_format="json",
            )
            record(GROUP, "create (setup for admin ops)", "n/a", r)
            key_id = _extract_id(r)
            if not key_id:
                print(f"iam api-keys admin setup ({fmt}): no key id extracted, skipping delete")
                continue
        finally:
            if key_id:
                r = run_cx(
                    ["iam", "api-keys", "admin", "delete", "--ids", key_id],
                    extra_flags=YES,
                    output_format=fmt,
                )
                record(GROUP, "api-keys admin delete", fmt, r)
                if r.get("exit_code") != 0:
                    print(
                        f"iam api-keys admin delete cleanup ({fmt}): non-zero exit "
                        f"(tolerated): {r.get('stderr', '')[:200]}"
                    )


def users_search():
    for fmt in FORMATS:
        r = run_cx(["iam", "users", "search"], output_format=fmt)
        record(GROUP, "users search", fmt, r)


def users_get_own():
    """Read-only lookup of this operator's own real account. `get` needs the
    numeric user_account_id (37832), not the UUID user_id search returns --
    confirmed working and safe (no test-user available to use instead)."""
    for fmt in FORMATS:
        r = run_cx(["iam", "users", "get", "37832"], output_format=fmt)
        record(GROUP, "users get", fmt, r)


def ip_access_flow():
    """`ip-access` is a single global, team-level security resource (no id
    param) -- not a per-run throwaway target. It's only safe to mutate
    because the original run found it empty and only ever added a single
    DISABLED (inactive) allow-style rule before deleting everything again.
    Re-check the live baseline is still empty before repeating that; if a
    real operator has since configured real IP restrictions, skip the
    mutating steps entirely rather than risk deleting them."""
    baseline = run_cx(["iam", "ip-access", "get"], output_format="json")
    baseline_empty = False
    try:
        obj = json.loads(baseline.get("stdout") or "{}")
        ip_access = (obj.get("settings") or {}).get("ipAccess")
        baseline_empty = not ip_access  # {}, None, [] are all "empty" here
    except (json.JSONDecodeError, ValueError):
        baseline_empty = False

    for fmt in FORMATS:
        r = run_cx(["iam", "ip-access", "get"], output_format=fmt)
        record(GROUP, "ip-access get", fmt, r)

    if not baseline_empty:
        record_skip(
            GROUP,
            "ip-access create/update/delete",
            "Baseline ip_access was non-empty on this run -- real IP "
            "restrictions may now be configured on this team. Skipping the "
            "create/update/delete mutation sequence to avoid touching them.",
        )
        return

    empty_payload = os.path.join(PAYLOADS_DIR, "iam_ipaccess_empty.json")
    single_rule_payload = os.path.join(PAYLOADS_DIR, "iam_ipaccess_single_disabled_rule.json")

    try:
        for fmt in FORMATS:
            r = run_cx(
                ["iam", "ip-access", "create", "--from-file", empty_payload],
                extra_flags=YES,
                output_format=fmt,
            )
            record(GROUP, "ip-access create", fmt, r)

        for fmt in FORMATS:
            r = run_cx(
                ["iam", "ip-access", "update", "--from-file", single_rule_payload],
                extra_flags=YES,
                output_format=fmt,
            )
            record(GROUP, "ip-access update", fmt, r)
    finally:
        for fmt in FORMATS:
            r = run_cx(["iam", "ip-access", "delete"], extra_flags=YES, output_format=fmt)
            record(GROUP, "ip-access delete (cleanup)", fmt, r)
            if r.get("exit_code") != 0:
                print(
                    f"iam ip-access cleanup ({fmt}): non-zero exit (tolerated): "
                    f"{r.get('stderr', '')[:200]}"
                )


def run():
    run_id = _run_id()
    roles_flow(run_id)
    groups_flow(run_id)
    scopes_readonly()
    api_keys_flow(run_id)
    api_keys_admin_flow(run_id)
    users_search()
    users_get_own()
    ip_access_flow()


if __name__ == "__main__":
    run()
