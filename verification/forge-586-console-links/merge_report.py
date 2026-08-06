"""
Combine all results/<group>.jsonl files into a single master HTML report.
Run this any time (safe to re-run repeatedly) to refresh the aggregate view.
Output: report.html in this directory.

The "Full results by command group" section is filtered down to only the subcommands
PR #176's own coverage table marks as expected to print a "View in Coralogix" console
link (see CANONICAL below), plus a small set of ✖ setup/cleanup rows that are necessary
lifecycle plumbing for a ✅ subcommand in the same group (see EXCEPTIONS). Groups/subfamilies
that are entirely ✖ are dropped in full, with no exceptions (e.g. `profiles`, `webhooks
actions/*`) — this mirrors the PR table itself, which never expects a link from them at all.
The underlying results/*.jsonl files are never modified; filtering happens only here, at
render time.
"""

import glob
import html
import json
import os
import re
import sys

BASE_DIR = os.path.dirname(os.path.abspath(__file__))
RESULTS_DIR = os.path.join(BASE_DIR, "results")
OUT_PATH = os.path.join(BASE_DIR, "report.html")

GROUP_ORDER = [
    "profiles", "completions", "cleanup", "schema", "dataprime", "docs", "search-fields",
    "logs", "metrics", "spans",
    "dashboards",
    "alerts", "suppression-rules", "cases",
    "notifications", "webhooks",
    "usage", "tco", "retentions",
    "e2m", "recording-rules", "parsing-rules", "enrichments",
    "integrations", "views",
    "iam",
    "archive", "ai-center", "slos", "olly",
]

# ---------------------------------------------------------------------------
# Canonical ✅/✖ table, transcribed from the PR #176 description's coverage table.
# True = expected to print a "View in Coralogix" console link. Keys are canonical
# slash-separated subcommand paths within each group (or the group's own name for
# single-leaf groups, e.g. "logs").
# ---------------------------------------------------------------------------
CANONICAL = {
    "profiles": {"list": False, "add": False, "delete": False, "set-default": False},
    "completions": {"generate": False, "install": False, "refresh": False},
    "cleanup": {"cleanup": False},
    "schema": {"schema": False},
    "dataprime": {"list": False, "show": False, "query": False},
    "docs": {"search": False, "fetch": False},
    "search-fields": {"search-fields": False, "semantic": False, "value": False},
    "logs": {"logs": False},
    "metrics": {"query": False, "query-range": False, "search": False, "get-labels": False},
    "spans": {"spans": False},

    "dashboards": {
        "get": True, "create": True, "replace": True, "check": True,
        "catalog": False, "delete": False, "search": False, "query-search": False,
        "folders/list": False, "folders/create": False, "folders/delete": False,
    },
    "alerts": {
        "get": True, "create": True, "enable": True, "disable": True,
        "list": False, "delete": False, "events": False, "event-stats": False,
    },
    "suppression-rules": {
        "create": True, "update": True,
        "list": False, "get": False, "delete": False,
    },
    "cases": {
        "get": True, "update": True, "comment": True, "assign": True, "unassign": True,
        "acknowledge": True, "unacknowledge": True, "resolve": True, "close": True,
        "set-priority": True, "clear-priority": True,
        "events/list": False, "events/get": False, "notifications": False,
    },
    "notifications": {
        "connectors/create": True, "connectors/update": True, "connectors/get": True,
        "connectors/list": False, "connectors/delete": False, "connectors/types": False,
        "connectors/entity-types": False, "connectors/entity-subtypes": False,
        "routers/create": True, "routers/update": True,
        "routers/list": False, "routers/get": False, "routers/delete": False,
        "routers/validate-matcher": False,
        "presets/list": False, "presets/get": False, "presets/create": False,
        "presets/update": False, "presets/delete": False, "presets/set-default": False,
        "test/connector": False, "test/destination": False, "test/preset": False,
        "test/routing-condition": False, "test/template-render": False,
    },
    "webhooks": {
        "list": True, "get": True, "create": True, "update": True, "delete": True,
        "test": True, "types": True,
        "actions/list": False, "actions/get": False, "actions/create": False,
        "actions/update": False, "actions/delete": False, "actions/batch": False,
        "actions/reorder": False,
    },
    "usage": {
        "summary": True, "daily": True, "logs-count": True, "spans-count": True,
        "capabilities": True, "query": True, "export-status": True,
    },
    "tco": {
        "list": True, "get": True, "create": True, "update": True, "delete": True,
        "reorder": True, "test": True, "settings": True, "settings-update": True,
    },
    "retentions": {"list": False, "update": False, "activate": False, "status": False},
    "e2m": {
        "create": True, "update": True,
        "list": False, "get": False, "delete": False, "labels-cardinality": False, "limits": False,
    },
    "recording-rules": {"list": True, "get": True, "create": True, "update": True, "delete": True},
    "parsing-rules": {
        "create": True, "update": True,
        "list": False, "get": False, "delete": False, "bulk-delete": False, "usage-limits": False,
    },
    "enrichments": {
        "list": True, "add": True, "remove": True, "overwrite": True, "limit": True, "settings": True,
        "custom/list": True, "custom/get": True, "custom/create": True, "custom/update": True,
        "custom/delete": True, "custom/search": True,
    },
    "integrations": {
        "list": True, "get": True, "definition": True, "deployed": True, "create": True,
        "update": True, "delete": True, "test": True, "template": True,
        "extensions/list": True, "extensions/get": True, "extensions/deployed": True,
        "extensions/deploy": True, "extensions/update": True, "extensions/undeploy": True,
        "contextual-data/list": True, "contextual-data/get": True, "contextual-data/create": True,
        "contextual-data/update": True, "contextual-data/delete": True,
        "contextual-data/definition": True, "contextual-data/test": True,
    },
    "views": {
        "create": True, "update": True,
        "list": False, "get": False, "delete": False,
        "folders/list": False, "folders/get": False, "folders/create": False,
        "folders/update": False, "folders/delete": False,
    },
    "iam": {
        "roles/create": True, "roles/update": True,
        "roles/list": False, "roles/get": False, "roles/delete": False, "roles/system": False,
        "scopes/create": True, "scopes/update": True,
        "scopes/list": False, "scopes/get": False, "scopes/delete": False,
        "groups/create": True, "groups/update": True,
        "groups/list": False, "groups/get": False, "groups/get-by-name": False,
        "groups/users": False, "groups/delete": False,
        "api-keys/list": True, "api-keys/get": True, "api-keys/create": True,
        "api-keys/update": True, "api-keys/delete": True, "api-keys/send-data-keys": True,
        "api-keys/admin/list": True, "api-keys/admin/delete": True, "api-keys/admin/set-status": True,
        "users/search": True, "users/get": True, "users/create": True,
        "users/update": True, "users/set-status": True,
        "ip-access/get": True, "ip-access/create": True, "ip-access/update": True, "ip-access/delete": True,
    },
    "archive": {
        "metrics/get": True, "metrics/create": True, "metrics/update": True,
        "metrics/enable": True, "metrics/disable": True, "metrics/validate": True,
        "logs/get": True, "logs/set": True,
    },
    "ai-center": {
        "applications/list": True, "applications/get": True,
        "evaluations/list": True, "evaluations/get": True, "evaluations/create": True,
        "evaluations/update": True, "evaluations/delete": True,
        "custom-evaluations/list": True, "custom-evaluations/list-for-application": True,
        "custom-evaluations/create": True, "custom-evaluations/update": True,
        "custom-evaluations/add": True, "custom-evaluations/remove": True,
        # Not a real PR-table row; the test agent's cleanup call for a custom-evaluation
        # entity used this verb instead of `remove` — kept via EXCEPTIONS as cleanup
        # plumbing, same as `remove`.
        "custom-evaluations/delete": False,
        "coverage": False, "model-pricing/get": False, "model-pricing/set": False,
    },
    "slos": {"create": True, "update": True, "list": False, "get": False, "delete": False},
    "olly": {"ask": True, "artifacts/list": False, "artifacts/get": False},
}

# (group, canonical_path) pairs kept despite being ✖: necessary create/delete lifecycle
# plumbing for a ✅ subcommand tested in the same (non-zero-✅) group. See plan doc for
# the reasoning on why `parsing-rules bulk-delete` is deliberately NOT here.
EXCEPTIONS = {
    ("alerts", "delete"),
    ("dashboards", "delete"),
    ("e2m", "delete"),
    ("parsing-rules", "delete"),
    ("suppression-rules", "delete"),
    ("views", "delete"),
    ("iam", "roles/delete"),
    ("iam", "scopes/delete"),
    ("iam", "groups/delete"),
    ("notifications", "connectors/delete"),
    ("notifications", "routers/delete"),
    ("slos", "delete"),
    ("ai-center", "custom-evaluations/delete"),
}

# Groups whose recorded subcommand strings redundantly repeat the group name as the
# first token (only webhooks does this).
STRIP_GROUP_PREFIX = {"webhooks"}

# Single-leaf groups: any recorded subcommand string maps to the group's own name.
SINGLE_LEAF_GROUPS = {"logs", "spans", "schema", "cleanup"}


def normalize(group, subcommand_str):
    """Map a raw recorded `subcommand` string to a canonical path, or None if it's not
    a real tested subcommand (e.g. a "NOTE: ..." narrative aside)."""
    s = subcommand_str.strip()
    if s.startswith("NOTE:"):
        return None

    if group in SINGLE_LEAF_GROUPS:
        return group

    # Drop parenthetical annotations: "(setup)", "(cleanup, ...)", "(attempt 1)", etc.
    s = re.split(r"\s*\(", s, maxsplit=1)[0].strip()

    if group in STRIP_GROUP_PREFIX and s.startswith(group + " "):
        s = s[len(group) + 1:]

    # Keep only leading path tokens: drop quoted example args and flags.
    tokens = []
    for tok in s.split():
        if tok.startswith("-") or tok.startswith("'") or tok.startswith('"'):
            break
        tokens.append(tok)
    if not tokens:
        return None

    if group == "search-fields":
        # "semantic ..." / "value ..." are search-fields' own flag-driven modes;
        # canonical table treats the whole group as a single ✖ leaf either way.
        return "search-fields"

    canon = "/".join(tokens)

    # Two api-keys setup calls in `iam` were recorded without their "api-keys" prefix
    # ("create (setup for get/update)", "create (setup for admin ops)") — context (the
    # note text itself) confirms these are api-keys/create, not a bare top-level create.
    if group == "iam" and canon == "create":
        canon = "api-keys/create"

    return canon


def subfamily(canonical_path):
    """First path segment if the path has one, else None (flat/top-level path)."""
    return canonical_path.split("/")[0] if "/" in canonical_path else None


def group_has_any_true(group):
    return any(CANONICAL.get(group, {}).values())


def subfamily_has_any_true(group, fam):
    prefix = fam + "/"
    return any(v for k, v in CANONICAL.get(group, {}).items() if k.startswith(prefix))


def should_keep(group, subcommand_str):
    canon = normalize(group, subcommand_str)
    if canon is None:
        return False, canon

    table = CANONICAL.get(group)
    if table is None or not group_has_any_true(group):
        return False, canon

    fam = subfamily(canon)
    if fam is not None and not subfamily_has_any_true(group, fam):
        return False, canon

    if canon not in table:
        print(f"UNMATCHED: {group} / {subcommand_str!r} -> {canon!r}", file=sys.stderr)
        return False, canon

    if table[canon]:
        return True, canon
    return (group, canon) in EXCEPTIONS, canon


def load(group):
    path = os.path.join(RESULTS_DIR, f"{group}.jsonl")
    if not os.path.exists(path):
        return []
    entries = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line:
                entries.append(json.loads(line))
    return entries


def badge(status):
    cls = {"PASS": "pass", "FAIL": "fail", "SKIPPED": "skip"}.get(status, "skip")
    return f'<span class="badge {cls}">{status}</span>'


# ---------------------------------------------------------------------------
# Curated qualitative findings surfaced by the 10 domain agents during this test run.
# tag: bug (PR176-relevant regression) | api (pre-existing backend/API limitation,
# unrelated to this PR) | residual (live-team state left changed) | handled (an issue
# that came up mid-run and was already resolved/reverted before this report was built)
# ---------------------------------------------------------------------------
FINDINGS = [
    {
        "tag": "fixed",
        "title": "dashboards replace printed no link when the API echoed back an empty body",
        "body": "The replace itself worked (confirmed via a follow-up get), but this demo team's replace "
                "endpoint returns an empty response with no id, so the CLI had nothing to embed a "
                "consoleUrl into and printed no “View in Coralogix” line — unlike create/get on the "
                "same dashboard. Fixed: run_replace now falls back to the dashboard id already known "
                "from the request payload when the response doesn't carry one "
                "(src/commands/dashboards/mod.rs). Covered by a new regression test in "
                "tests/console_urls/main.rs; full suite, cargo fmt, and cargo clippy -D warnings all "
                "clean.",
    },
    {
        "tag": "fixed",
        "title": "views create didn't surface the created view or its link",
        "body": "text mode printed nothing at all; json/agents printed an empty list instead of the "
                "created object. The view was genuinely created server-side (confirmed via list), and "
                "views update on the same id correctly showed both the object and its consoleUrl — the "
                "bug was in create's response handling specifically: ViewsApi::create required the "
                "response wrapped as {\"view\": {...}} and failed closed (empty result) whenever the "
                "API didn't return that exact shape. Fixed: create now returns the raw response like "
                "get/update always have, with a new view_id_from_response() helper that tries both the "
                "wrapped and bare shapes (src/commands/views/api.rs, src/commands/views/mod.rs). Covered "
                "by 4 new unit tests plus a new bare-response regression test in "
                "tests/console_urls/main.rs; full suite, cargo fmt, and cargo clippy -D warnings all "
                "clean.",
    },
    {
        "tag": "fixed",
        "title": "webhooks create printed [] and no console link",
        "body": "Same shape as the views issue above and same root cause: the webhook was genuinely "
                "created (confirmed via list), but WebhooksApi::create required the response wrapped as "
                "{\"webhook\": {...}} and silently dropped the whole row when the live API didn't return "
                "that envelope, while list/get/update/delete on the same webhook all worked correctly. "
                "Fixed identically to views create — raw response + defensive webhook_id_from_response() "
                "helper (src/commands/webhooks/api.rs, src/commands/webhooks/mod.rs). Covered by 4 new "
                "unit tests plus a new bare-response regression test in tests/console_urls/main.rs; full "
                "suite, cargo fmt, and cargo clippy -D warnings all clean.",
    },
    {
        "tag": "corrected",
        "title": "iam users search only embedding consoleUrl on the first row is not a bug",
        "body": "Originally flagged as a bug (reproduced against 8 real users, every row after the first "
                "missing consoleUrl in both json and agents formats), but code review found it's the "
                "same documented “tag only the first row per profile” pattern used by every other list "
                "command in this codebase (identical code path to webhooks list / api-keys list, both "
                "confirmed working correctly). With a single profile, only row 0 getting consoleUrl is "
                "the intended behavior, not a divergence — no fix needed.",
    },
    {
        "tag": "api",
        "title": "Several pre-existing backend/API issues, unrelated to this PR",
        "body": "docs search/fetch: 100% HTTP 403 from Cloudflare, likely the CLI's User-Agent getting "
                "blocked (same URLs return 200/404 via curl). completions generate elvish: fails even "
                "though elvish is a listed valid --help value. enrichments overwrite: rejects a "
                "well-formed payload with “enrichmentType is required.” enrichments remove: silent "
                "no-op — returns success but never deletes (7+ payload shapes tried). connectors "
                "entity-types/entity-subtypes: 404. iam groups users: 404 on every group tested, "
                "including one with real members. iam api-keys list (non-admin): 400, looks like the "
                "literal string “list” leaking into a key_id parameter. iam api-keys admin set-status: "
                "400 unknown field “keyIds”, looks like a camelCase/snake_case mismatch. "
                "recording-rules create: reports success but the rule group never appears via list/get. "
                "integrations create/update/delete/test, webhooks update/test, tco settings-update, "
                "retentions update: all return 501 Unimplemented on this demo team's backend.",
    },
    {
        "tag": "residual",
        "title": "Four settings left in a different state on the live kb-demo team",
        "body": "retentions.enableTags flipped false→true via activate; the CLI's own retentions update "
                "path returned 501 Unimplemented when reversion was attempted, so it stayed flipped. "
                "archive.archiveSpec.enableTags similarly flipped false→true as a side effect of the "
                "first successful logs set call — every field-name guess for reverting it was rejected "
                "as unknown. One ai-center custom-evaluation test policy remains attached to no "
                "application (this CLI has no delete route for that entity type, by design). Two "
                "enrichment stubs (ids 244123 / 244124, field cxclipr176testfield) are stuck due to the "
                "silent-no-op remove bug above — harmless since the field doesn't exist in real data, "
                "but would need manual removal in the Coralogix UI.",
    },
    {
        "tag": "handled",
        "title": "Three real pipeline-generated cases were resolved/closed mid-test, then partially reverted",
        "body": "cases has no create and no list — the only way to exercise its lifecycle subcommands "
                "was against real, already-existing cases. CASE-43, CASE-96, and CASE-97 (all real, "
                "alert-generated cases on kb-demo) ended up reassigned, reprioritized, retitled, and "
                "closed over the course of testing every lifecycle subcommand. After review, titles were "
                "reverted to their originals and priorities were already back to their original values "
                "by the time testing finished. Status cannot be reverted through this CLI — there is no "
                "reopen/unclose subcommand — so all three remain CLOSED; reopen manually in the "
                "Coralogix UI if that's not acceptable.",
    },
    {
        "tag": "handled",
        "title": "A real API key secret was briefly written to disk, then caught and revoked",
        "body": "While testing iam api-keys create/get, plaintext key values were persisted unredacted "
                "into results/iam.jsonl for a few entries before being caught mid-run. A full-corpus scan "
                "across every results file (run independently, after the fact) confirmed no secret "
                "material remains anywhere in this report's data, and the exposed keys were revoked.",
    },
]

TAG_META = {
    "bug": ("PR176 bug", "var(--fail)"),
    "fixed": ("Fixed", "var(--pass)"),
    "corrected": ("Not a bug — verified", "var(--accent)"),
    "api": ("Backend/API limitation", "var(--skip)"),
    "residual": ("Residual live-team state", "var(--skip)"),
    "handled": ("Handled during this run", "var(--accent)"),
}


def main():
    all_groups = {}
    seen = set()
    for g in GROUP_ORDER:
        entries = load(g)
        if entries:
            all_groups[g] = entries
        seen.add(g)
    for path in sorted(glob.glob(os.path.join(RESULTS_DIR, "*.jsonl"))):
        g = os.path.basename(path)[: -len(".jsonl")]
        if g not in seen:
            all_groups[g] = load(g)

    total = {"PASS": 0, "FAIL": 0, "SKIPPED": 0}
    sections = []
    nav = []
    dropped_groups = []

    for g, entries in all_groups.items():
        filtered = [e for e in entries if should_keep(g, e["subcommand"])[0]]
        if not filtered:
            dropped_groups.append(g)
            continue

        counts = {"PASS": 0, "FAIL": 0, "SKIPPED": 0}
        rows = []
        for e in filtered:
            status = e.get("status", "FAIL")
            counts[status] = counts.get(status, 0) + 1
            total[status] = total.get(status, 0) + 1

            stdout_excerpt = (e.get("stdout") or "")
            stderr_excerpt = (e.get("stderr") or "")
            if len(stdout_excerpt) > 3000:
                stdout_excerpt = stdout_excerpt[:3000] + "\n... [truncated]"
            if len(stderr_excerpt) > 2000:
                stderr_excerpt = stderr_excerpt[:2000] + "\n... [truncated]"

            rows.append(f"""
            <tr>
              <td class="mono">{html.escape(e.get('subcommand',''))}</td>
              <td class="mono dim">{html.escape(str(e.get('output_format','')))}</td>
              <td>{badge(status)}</td>
              <td class="mono num">{html.escape(str(e.get('exit_code')) if e.get('exit_code') is not None else '—')}</td>
              <td><details><summary>cmd</summary><pre>{html.escape(e.get('command',''))}</pre></details></td>
              <td><details><summary>stdout</summary><pre>{html.escape(stdout_excerpt)}</pre></details></td>
              <td><details><summary>stderr</summary><pre>{html.escape(stderr_excerpt)}</pre></details></td>
              <td class="notes">{html.escape(e.get('notes',''))}</td>
            </tr>""")

        anchor = g.replace(" ", "-")
        dominant = "fail" if counts.get("FAIL", 0) else ("skip" if counts.get("SKIPPED", 0) else "pass")
        nav.append(f"""<a class="chip {dominant}" href="#{anchor}">
            <span class="chip-label">{html.escape(g)}</span>
            <span class="chip-counts">{counts.get('PASS',0)}&middot;{counts.get('FAIL',0)}&middot;{counts.get('SKIPPED',0)}</span>
        </a>""")

        sections.append(f"""
        <section class="panel" id="{anchor}">
          <div class="panel-head">
            <h2>{html.escape(g)}</h2>
            <div class="panel-counts">
              <span class="count pass">{counts.get('PASS',0)} pass</span>
              <span class="count fail">{counts.get('FAIL',0)} fail</span>
              <span class="count skip">{counts.get('SKIPPED',0)} skip</span>
              <span class="count total">{len(filtered)} total</span>
            </div>
          </div>
          <div class="table-scroll">
            <table>
            <thead><tr><th>Subcommand</th><th>Format</th><th>Status</th><th>Exit</th><th>Command</th><th>Stdout</th><th>Stderr</th><th>Notes</th></tr></thead>
            <tbody>
            {''.join(rows)}
            </tbody>
            </table>
          </div>
        </section>""")

    findings_html = []
    for f in FINDINGS:
        label, color = TAG_META[f["tag"]]
        findings_html.append(f"""
        <li class="finding finding-{f['tag']}">
          <span class="finding-tag" style="--tag-color:{color}">{label}</span>
          <h3>{html.escape(f['title'])}</h3>
          <p>{f['body']}</p>
        </li>""")

    grand_total = sum(total.values())
    pass_pct = round(100 * total.get("PASS", 0) / grand_total) if grand_total else 0

    out = f"""<!doctype html><html><head><meta charset="utf-8">
<title>cx CLI &mdash; PR #176 command coverage test</title>
<style>
:root {{
  --bg: #f5f7fb;
  --surface: #ffffff;
  --surface-2: #eef1f7;
  --border: #d8dee9;
  --text: #1b2233;
  --text-dim: #5b6478;
  --accent: #0f8a7a;
  --accent-bg: #e3f5f1;
  --pass: #1a7f37;
  --pass-bg: #dcf5e2;
  --fail: #cf222e;
  --fail-bg: #fce4e4;
  --skip: #9a6700;
  --skip-bg: #faf0d6;
  color-scheme: light dark;
}}
@media (prefers-color-scheme: dark) {{
  :root {{
    --bg: #0b0f16;
    --surface: #121826;
    --surface-2: #17203133;
    --border: #26304a;
    --text: #dbe2f0;
    --text-dim: #8996b3;
    --accent: #5fd4c4;
    --accent-bg: #16342f;
    --pass: #4ce06b;
    --pass-bg: #123a1d;
    --fail: #ff6b64;
    --fail-bg: #3a1414;
    --skip: #e8b649;
    --skip-bg: #3a2c0c;
  }}
}}
:root[data-theme="dark"] {{
  --bg: #0b0f16; --surface: #121826; --surface-2: #17203133; --border: #26304a;
  --text: #dbe2f0; --text-dim: #8996b3; --accent: #5fd4c4; --accent-bg: #16342f;
  --pass: #4ce06b; --pass-bg: #123a1d; --fail: #ff6b64; --fail-bg: #3a1414;
  --skip: #e8b649; --skip-bg: #3a2c0c;
}}
:root[data-theme="light"] {{
  --bg: #f5f7fb; --surface: #ffffff; --surface-2: #eef1f7; --border: #d8dee9;
  --text: #1b2233; --text-dim: #5b6478; --accent: #0f8a7a; --accent-bg: #e3f5f1;
  --pass: #1a7f37; --pass-bg: #dcf5e2; --fail: #cf222e; --fail-bg: #fce4e4;
  --skip: #9a6700; --skip-bg: #faf0d6;
}}

* {{ box-sizing: border-box; }}
html, body {{ overflow-x: hidden; }}
body {{
  margin: 0;
  background: var(--bg);
  color: var(--text);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  font-size: 15px;
  line-height: 1.55;
}}
.mono {{ font-family: ui-monospace, "JetBrains Mono", "SF Mono", "Cascadia Code", Menlo, Consolas, monospace; }}
.num {{ font-variant-numeric: tabular-nums; }}
.dim {{ color: var(--text-dim); }}

header {{
  position: sticky;
  top: 0;
  z-index: 5;
  background: var(--bg);
  border-bottom: 1px solid var(--border);
  padding: 20px clamp(16px, 4vw, 40px) 14px;
}}
.title-row {{
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 8px 24px;
}}
h1 {{
  font-family: ui-monospace, "JetBrains Mono", "SF Mono", "Cascadia Code", Menlo, Consolas, monospace;
  font-size: 20px;
  font-weight: 600;
  margin: 0;
  letter-spacing: -0.01em;
  text-wrap: balance;
}}
h1 .dim-part {{ color: var(--text-dim); font-weight: 500; }}
.subtitle {{
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--text-dim);
}}
.stat-row {{
  display: flex;
  gap: 18px;
  align-items: baseline;
  font-family: ui-monospace, "JetBrains Mono", "SF Mono", "Cascadia Code", Menlo, Consolas, monospace;
}}
.stat {{ display: flex; flex-direction: column; align-items: flex-end; }}
.stat .n {{ font-size: 22px; font-weight: 700; font-variant-numeric: tabular-nums; }}
.stat .l {{ font-size: 10px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--text-dim); }}
.stat.pass .n {{ color: var(--pass); }}
.stat.fail .n {{ color: var(--fail); }}
.stat.skip .n {{ color: var(--skip); }}

.chiprow {{
  margin-top: 14px;
  display: flex;
  gap: 8px;
  overflow-x: auto;
  padding-bottom: 4px;
}}
.chip {{
  flex: 0 0 auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 6px 10px;
  border-radius: 3px;
  border: 1px solid var(--border);
  border-left: 3px solid var(--text-dim);
  background: var(--surface);
  text-decoration: none;
  color: var(--text);
  font-family: ui-monospace, "JetBrains Mono", "SF Mono", "Cascadia Code", Menlo, Consolas, monospace;
  font-size: 11px;
  white-space: nowrap;
}}
.chip.pass {{ border-left-color: var(--pass); }}
.chip.fail {{ border-left-color: var(--fail); }}
.chip.skip {{ border-left-color: var(--skip); }}
.chip-label {{ font-weight: 600; }}
.chip-counts {{ color: var(--text-dim); font-variant-numeric: tabular-nums; }}

main {{ padding: 24px clamp(16px, 4vw, 40px) 80px; max-width: 1400px; margin: 0 auto; }}

.findings {{
  margin: 0 0 40px;
  padding: 0;
  list-style: none;
  display: grid;
  gap: 10px;
}}
.finding {{
  background: var(--surface);
  border: 1px solid var(--border);
  border-left: 3px solid var(--tag-color, var(--border));
  border-radius: 3px;
  padding: 14px 16px;
}}
.finding-tag {{
  display: inline-block;
  font-family: ui-monospace, "JetBrains Mono", "SF Mono", "Cascadia Code", Menlo, Consolas, monospace;
  font-size: 10px;
  text-transform: uppercase;
  letter-spacing: 0.07em;
  color: var(--tag-color);
  border: 1px solid var(--tag-color);
  border-radius: 2px;
  padding: 1px 6px;
  margin-bottom: 8px;
}}
.finding h3 {{ margin: 0 0 6px; font-size: 15px; font-weight: 650; text-wrap: balance; }}
.finding p {{ margin: 0; color: var(--text-dim); max-width: 78ch; font-size: 13.5px; }}

h2.section-label {{
  font-family: ui-monospace, "JetBrains Mono", "SF Mono", "Cascadia Code", Menlo, Consolas, monospace;
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.09em;
  color: var(--text-dim);
  margin: 0 0 14px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border);
}}

.panel {{
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 4px;
  margin-bottom: 20px;
  scroll-margin-top: 190px;
  overflow: hidden;
}}
.panel-head {{
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 6px 16px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border);
  background: var(--surface-2);
}}
.panel-head h2 {{
  font-family: ui-monospace, "JetBrains Mono", "SF Mono", "Cascadia Code", Menlo, Consolas, monospace;
  font-size: 14px;
  font-weight: 700;
  margin: 0;
}}
.panel-counts {{ display: flex; gap: 12px; font-size: 11px; font-family: ui-monospace, monospace; }}
.count {{ font-variant-numeric: tabular-nums; }}
.count.pass {{ color: var(--pass); }}
.count.fail {{ color: var(--fail); }}
.count.skip {{ color: var(--skip); }}
.count.total {{ color: var(--text-dim); }}

.table-scroll {{ overflow-x: auto; }}
table {{ border-collapse: collapse; width: 100%; font-size: 12.5px; }}
thead th {{
  position: sticky; top: 0;
  background: var(--surface-2);
  text-align: left;
  font-weight: 600;
  padding: 7px 10px;
  border-bottom: 1px solid var(--border);
  white-space: nowrap;
}}
tbody td {{
  padding: 6px 10px;
  border-bottom: 1px solid var(--border);
  vertical-align: top;
}}
tbody tr:last-child td {{ border-bottom: none; }}
tbody tr:hover {{ background: var(--surface-2); }}

.badge {{
  display: inline-block;
  padding: 1px 7px;
  border-radius: 2px;
  font-size: 10.5px;
  font-weight: 700;
  font-family: ui-monospace, monospace;
  letter-spacing: 0.02em;
}}
.badge.pass {{ color: var(--pass); background: var(--pass-bg); }}
.badge.fail {{ color: var(--fail); background: var(--fail-bg); }}
.badge.skip {{ color: var(--skip); background: var(--skip-bg); }}

details summary {{
  cursor: pointer;
  color: var(--accent);
  font-family: ui-monospace, monospace;
  font-size: 11px;
  list-style: none;
}}
details summary::-webkit-details-marker {{ display: none; }}
details summary:before {{ content: "\\25b8\\a0"; }}
details[open] summary:before {{ content: "\\25be\\a0"; }}
details pre {{
  margin: 6px 0 0;
  padding: 8px 10px;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 3px;
  white-space: pre-wrap;
  word-break: break-word;
  max-width: 520px;
  font-size: 11px;
  font-family: ui-monospace, "JetBrains Mono", "SF Mono", Menlo, Consolas, monospace;
  color: var(--text-dim);
}}
.notes {{ color: var(--text-dim); font-size: 12px; max-width: 260px; }}

a {{ color: var(--accent); }}
::selection {{ background: var(--accent-bg); }}
</style></head><body>
<header>
  <div class="title-row">
    <div>
      <h1>cx <span class="dim-part">&mdash; PR #176 command coverage test</span></h1>
      <p class="subtitle">Filtered to subcommands the PR's coverage table marks as console-link-eligible ({len(dropped_groups)} group{'s' if len(dropped_groups) != 1 else ''} with none removed entirely)</p>
    </div>
    <div class="stat-row">
      <div class="stat pass"><span class="n num">{total.get('PASS',0)}</span><span class="l">pass</span></div>
      <div class="stat fail"><span class="n num">{total.get('FAIL',0)}</span><span class="l">fail</span></div>
      <div class="stat skip"><span class="n num">{total.get('SKIPPED',0)}</span><span class="l">skip</span></div>
      <div class="stat"><span class="n num">{grand_total}</span><span class="l">total &middot; {pass_pct}%</span></div>
    </div>
  </div>
  <nav class="chiprow">{''.join(nav)}</nav>
</header>
<main>
  <h2 class="section-label">Notable findings</h2>
  <ul class="findings">{''.join(findings_html)}</ul>

  <h2 class="section-label">Full results by command group</h2>
  {''.join(sections)}
</main>
</body></html>"""

    with open(OUT_PATH, "w") as f:
        f.write(out)
    print(f"Wrote {OUT_PATH} ({grand_total} results across {len(all_groups) - len(dropped_groups)} groups; "
          f"{len(dropped_groups)} groups fully dropped: {', '.join(dropped_groups)})")


if __name__ == "__main__":
    main()
