#!/usr/bin/env python3
"""Verify "View in Coralogix" console links against a real, live Coralogix team.

Why this script exists
-----------------------
`cx` (this repo) prints a `View in Coralogix: <url>` line on stderr - and, for
`-o json`/`-o agents`, embeds the same URL as a `consoleUrl` field - after
most mutation/read commands (see `docs/configuration.md#console-links` and
`src/console_url.rs`). All of that logic is unit- and wiremock-tested, but
none of those tests can catch a *systematically* wrong host/path, a route
that has moved in the real web console, or a real team whose
`/identity/whoami` response doesn't have the shape the code expects (see
`src/identity.rs`). Those things can only be caught by actually running `cx`
against a live team and inspecting the URLs it produces.

This script drives the real `cx` binary against a real Coralogix team,
captures the URL from both stderr and `-o json` output for every `cx`
subcommand family that can print a console link, and asserts it has the
expected shape (right domain, right hash-route, right entity ID).

See README.md in this directory for full usage instructions. Short version:

    export CX_API_KEY=...        # or CX_PROFILE, or a ~/.cx profile
    export CX_REGION=eu2         # whichever region your test team is on
    python3 scripts/verify_console_urls/verify_console_urls.py

This script is a development/verification aid, not part of the shipped `cx`
product - it is never invoked by `cx` itself, CI, or any release process.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.parse
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Optional

SCRIPT_DIR = Path(__file__).resolve().parent
FIXTURES_DIR = SCRIPT_DIR / "fixtures"

VIEW_IN_CORALOGIX_RE = re.compile(r"View in Coralogix:\s*(\S+)")

# ---------------------------------------------------------------------------
# Small result/reporting plumbing
# ---------------------------------------------------------------------------


class Status:
    PASS = "PASS"
    FAIL = "FAIL"
    SKIP = "SKIP"
    ERROR = "ERROR"


@dataclass
class Result:
    name: str
    status: str
    detail: str = ""


@dataclass
class Report:
    results: list = field(default_factory=list)

    def add(self, result: Result) -> None:
        self.results.append(result)
        marker = {
            Status.PASS: "PASS ",
            Status.FAIL: "FAIL ",
            Status.SKIP: "SKIP ",
            Status.ERROR: "ERROR",
        }[result.status]
        print(f"[{marker}] {result.name}")
        if result.detail:
            for line in result.detail.rstrip("\n").splitlines():
                print(f"         {line}")

    def summary_and_exit_code(self) -> int:
        counts = {Status.PASS: 0, Status.FAIL: 0, Status.SKIP: 0, Status.ERROR: 0}
        for r in self.results:
            counts[r.status] += 1
        print()
        print(
            f"Summary: {counts[Status.PASS]} passed, {counts[Status.FAIL]} failed, "
            f"{counts[Status.ERROR]} errored, {counts[Status.SKIP]} skipped "
            f"(of {len(self.results)} checks)"
        )
        if counts[Status.FAIL] or counts[Status.ERROR]:
            return 1
        return 0


# ---------------------------------------------------------------------------
# cx subprocess helpers
# ---------------------------------------------------------------------------


def find_cx_bin(explicit: Optional[str]) -> str:
    if explicit:
        return explicit
    for candidate in (
        shutil.which("cx"),
        str(SCRIPT_DIR / "../../target/debug/cx"),
        str(SCRIPT_DIR / "../../target/release/cx"),
    ):
        if candidate and Path(candidate).is_file():
            return candidate
        if candidate and shutil.which(candidate):
            return candidate
    raise SystemExit(
        "Could not find a `cx` binary. Build one with `cargo build` from the "
        "repo root, or pass --cx-bin /path/to/cx."
    )


@dataclass
class CxRun:
    args: list
    returncode: int
    stdout: str
    stderr: str

    @property
    def console_link_line(self) -> Optional[str]:
        m = VIEW_IN_CORALOGIX_RE.search(self.stderr)
        return m.group(1) if m else None

    def json(self):
        try:
            return json.loads(self.stdout)
        except (json.JSONDecodeError, ValueError):
            return None


def run_cx(cx_bin: str, profile: Optional[str], args: list, timeout: int = 60) -> CxRun:
    full_args = [cx_bin]
    if profile:
        full_args += ["-p", profile]
    full_args += args
    proc = subprocess.run(
        full_args,
        capture_output=True,
        text=True,
        timeout=timeout,
    )
    return CxRun(args=full_args, returncode=proc.returncode, stdout=proc.stdout, stderr=proc.stderr)


def find_console_url(val) -> Optional[str]:
    """Find a `consoleUrl` field in a JSON value - at the top level of an
    object, or on the first element of an array (matching the "first row
    only" tagging behavior fixed in this PR for list/multi-row output)."""
    if isinstance(val, dict):
        if isinstance(val.get("consoleUrl"), str):
            return val["consoleUrl"]
        # some responses nest the tagged object one level down
        for v in val.values():
            if isinstance(v, dict) and isinstance(v.get("consoleUrl"), str):
                return v["consoleUrl"]
    elif isinstance(val, list) and val:
        return find_console_url(val[0])
    return None


ID_KEYS = ("id", "roleId", "scopeId", "groupId", "alertId", "viewId", "ruleId", "connectorId", "routerId")


def find_id(val, depth: int = 0) -> Optional[str]:
    """Best-effort extraction of an entity ID from a `create` response.

    Tries the well-known top-level key names first, then recurses one level
    into nested objects (covers wrapper shapes like `{"dashboard": {...}}`).
    """
    if not isinstance(val, dict) or depth > 2:
        return None
    for key in ID_KEYS:
        if key in val and isinstance(val[key], (str, int)):
            return str(val[key])
    for v in val.values():
        if isinstance(v, dict):
            found = find_id(v, depth + 1)
            if found:
                return found
    return None


def check_url_fragment(name: str, url: Optional[str], expected_fragment: str, report: Report, source: str) -> bool:
    if url is None:
        report.add(Result(name, Status.FAIL, f"no console URL found in {source}"))
        return False
    if not url.startswith("https://") and not url.startswith("http://"):
        report.add(Result(name, Status.FAIL, f"{source} URL doesn't look like a URL: {url}"))
        return False
    if expected_fragment not in url:
        report.add(
            Result(
                name,
                Status.FAIL,
                f"{source} URL missing expected fragment {expected_fragment!r}\n  got: {url}",
            )
        )
        return False
    return True


def verify_link(name: str, run: CxRun, expected_fragment: str, report: Report) -> bool:
    """Verify both the stderr `View in Coralogix:` line and the `-o json`
    `consoleUrl` field independently, and that they agree with each other."""
    stderr_url = run.console_link_line
    json_val = run.json()
    json_url = find_console_url(json_val) if json_val is not None else None

    ok_stderr = check_url_fragment(name, stderr_url, expected_fragment, report, "stderr")
    ok_json = check_url_fragment(name, json_url, expected_fragment, report, "-o json")

    if ok_stderr and ok_json and stderr_url != json_url:
        report.add(
            Result(
                name,
                Status.FAIL,
                f"stderr and -o json disagree:\n  stderr: {stderr_url}\n  json:   {json_url}",
            )
        )
        return False

    if ok_stderr and ok_json:
        report.add(Result(name, Status.PASS, stderr_url))
        return True
    return False


# ---------------------------------------------------------------------------
# Static (page-level) checks - no entity to create, the link is the same
# regardless of which item you're looking at.
# ---------------------------------------------------------------------------

STATIC_CHECKS = [
    ("usage", ["usage", "summary"], "/#/settings/datausage"),
    ("tco", ["tco", "list"], "/#/tco-policies"),
    ("archive-logs", ["archive", "logs", "get"], "/#/physical-locations"),
    ("archive-metrics", ["archive", "metrics", "get"], "/#/physical-locations"),
    ("recording-rules", ["recording-rules", "list"], "/#/recording-rules"),
    ("enrichments", ["enrichments", "list"], "/#/enrichments"),
    ("enrichments-custom-tables", ["enrichments", "custom", "list"], "/#/enrichments"),
    ("integrations", ["integrations", "list"], "/#/extensions/integrations"),
    ("integrations-extensions", ["integrations", "extensions", "list"], "/#/extensions/integrations"),
    ("integrations-contextual-data", ["integrations", "contextual-data", "list"], "/#/extensions/integrations"),
    ("webhooks", ["webhooks", "list"], "/#/extensions/outbound-webhooks"),
    ("iam-api-keys", ["iam", "api-keys", "list"], "/#/settings/api-keys"),
    ("iam-users", ["iam", "users", "search"], "/#/settings/team/members"),
    ("iam-ip-access", ["iam", "ip-access", "get"], "/#/settings/login-access-policies"),
    ("ai-center-applications", ["ai-center", "applications", "list"], "/#/ai-center/overview/application-catalog"),
    ("ai-center-evaluations", ["ai-center", "evaluations", "list"], "/#/ai-center/overview/eval-catalog"),
]


def run_static_checks(cx_bin: str, profile: Optional[str], only: Optional[set], report: Report) -> None:
    for name, args, fragment in STATIC_CHECKS:
        if only and name not in only:
            continue
        run = run_cx(cx_bin, profile, args + ["-o", "json"])
        if run.returncode != 0:
            report.add(
                Result(
                    name,
                    Status.ERROR,
                    f"`cx {' '.join(args)}` exited {run.returncode}\nstderr: {run.stderr.strip()}",
                )
            )
            continue
        verify_link(name, run, fragment, report)


def run_olly_check(cx_bin: str, profile: Optional[str], include_olly: bool, only: Optional[set], report: Report) -> None:
    """`cx olly ask` is deliberately excluded from STATIC_CHECKS: unlike every
    other read-only check above, it triggers a real AI-assistant call (real
    latency, real cost, non-deterministic response) rather than a cheap list/
    get. Only run it if the caller explicitly opts in with --include-olly."""
    name = "olly"
    if only and name not in only:
        return
    if not include_olly:
        report.add(
            Result(
                name,
                Status.SKIP,
                "`cx olly ask` triggers a real AI-assistant call (cost + latency) - "
                "pass --include-olly to exercise it.",
            )
        )
        return
    run = run_cx(
        cx_bin,
        profile,
        ["olly", "ask", "console-url-check: please reply with a single word.", "--yes", "-o", "json"],
        timeout=120,
    )
    if run.returncode != 0:
        report.add(
            Result(name, Status.ERROR, f"`cx olly ask` exited {run.returncode}\nstderr: {run.stderr.strip()}")
        )
        return
    verify_link(name, run, "/#/olly", report)


# ---------------------------------------------------------------------------
# Entity-level checks - create a throwaway object, verify the link, delete it.
# ---------------------------------------------------------------------------


@dataclass
class EntityCheck:
    name: str
    create_path: list  # e.g. ["dashboards"] for `cx dashboards create ...`
    delete_path: list  # e.g. ["dashboards"] for `cx dashboards delete <id> --yes`
    fixture_file: str  # filename under fixtures/ (or --fixtures-dir)
    expected_fragment: Callable[[str], str]  # given the id, the expected URL fragment
    confidence: str  # "confirmed" | "best-effort" | "requires-fixture"
    id_is_query_encoded: bool = False


def frag_path(prefix: str) -> Callable[[str], str]:
    return lambda entity_id: f"{prefix}{entity_id}"


def frag_query(prefix: str) -> Callable[[str], str]:
    def _f(entity_id: str) -> str:
        encoded = urllib.parse.quote_plus(entity_id)
        return f"{prefix}{encoded}"

    return _f


ENTITY_CHECKS = [
    EntityCheck(
        "alerts",
        ["alerts"],
        ["alerts"],
        "alert.json",
        frag_path("/#/alerts/"),
        confidence="confirmed",
    ),
    EntityCheck(
        "dashboards",
        ["dashboards"],
        ["dashboards"],
        "dashboard.json",
        frag_path("/#/dashboards/"),
        confidence="confirmed",
    ),
    EntityCheck(
        "e2m",
        ["e2m"],
        ["e2m"],
        "e2m.json",
        frag_path("/#/tco/metrics/"),
        confidence="confirmed",
    ),
    EntityCheck(
        "iam-roles",
        ["iam", "roles"],
        ["iam", "roles"],
        "role.json",
        frag_query("/#/settings/roles?selectedRoleId="),
        confidence="best-effort",
    ),
    EntityCheck(
        "iam-scopes",
        ["iam", "scopes"],
        ["iam", "scopes"],
        "scope.json",
        frag_query("/#/settings/scopes?selectedScopeId="),
        confidence="best-effort",
    ),
    EntityCheck(
        "parsing-rules",
        ["parsing-rules"],
        ["parsing-rules"],
        "parsing_rule.json",
        frag_path("/#/rules/group/"),
        confidence="best-effort",
    ),
    EntityCheck(
        "views",
        ["views"],
        ["views"],
        "view.json",
        frag_query("/#/explore?viewId="),
        confidence="requires-fixture",
    ),
    EntityCheck(
        "slos",
        ["slos"],
        ["slos"],
        "slo.json",
        lambda entity_id: f"/#/slo/{entity_id}/overview",
        confidence="requires-fixture",
    ),
    EntityCheck(
        "suppression-rules",
        ["alerts", "suppression-rules"],
        ["alerts", "suppression-rules"],
        "suppression_rule.json",
        frag_query("/#/suppression-rules?edit="),
        confidence="requires-fixture",
    ),
    EntityCheck(
        "notification-connectors",
        ["notifications", "connectors"],
        ["notifications", "connectors"],
        "connector.json",
        frag_query("/#/notification-center/connectors?id="),
        confidence="requires-fixture",
    ),
    EntityCheck(
        "notification-routers",
        ["notifications", "routers"],
        ["notifications", "routers"],
        "router.json",
        frag_query("/#/notification-center/routers?id="),
        confidence="requires-fixture",
    ),
    EntityCheck(
        "iam-groups",
        ["iam", "groups"],
        ["iam", "groups"],
        "group.json",
        frag_query("/#/settings/account/groups?selectedGroupId="),
        confidence="requires-fixture",
    ),
]


def load_fixture(fixtures_dir: Path, filename: str) -> Optional[dict]:
    path = fixtures_dir / filename
    if not path.is_file():
        return None
    return json.loads(path.read_text())


def uniquify(payload: dict) -> dict:
    """Append a short random suffix to whichever name-ish field is present,
    so repeated runs (or runs against a team that already ran this script)
    don't collide on name uniqueness constraints some APIs enforce."""
    suffix = uuid.uuid4().hex[:8]
    payload = json.loads(json.dumps(payload))  # deep copy
    for key in ("name", "displayName"):
        if key in payload and isinstance(payload[key], str):
            payload[key] = f"{payload[key]} {suffix}"
    return payload


def run_entity_check(
    cx_bin: str,
    profile: Optional[str],
    check: EntityCheck,
    fixtures_dir: Path,
    only: Optional[set],
    keep: bool,
    report: Report,
) -> None:
    if only and check.name not in only:
        return

    fixture = load_fixture(fixtures_dir, check.fixture_file)
    if fixture is None:
        instructions = FIXTURE_INSTRUCTIONS.get(check.name, "See README.md for how to build this fixture.")
        report.add(
            Result(
                check.name,
                Status.SKIP,
                f"no fixture at {fixtures_dir / check.fixture_file} - {instructions}",
            )
        )
        return

    payload = uniquify(fixture)

    with tempfile.NamedTemporaryFile(
        "w", suffix=".json", prefix=f"cx-console-url-check-{check.name}-", delete=False
    ) as f:
        json.dump(payload, f)
        tmp_path = f.name

    entity_id: Optional[str] = None
    try:
        create_run = run_cx(
            cx_bin,
            profile,
            check.create_path + ["create", "--from-file", tmp_path, "--yes", "-o", "json"],
        )
        if create_run.returncode != 0:
            note = " (best-effort fixture - a real API validation error here may just mean the fixture needs adjusting, not that the console-link feature is broken)" if check.confidence == "best-effort" else ""
            report.add(
                Result(
                    check.name,
                    Status.ERROR,
                    f"`cx {' '.join(check.create_path)} create` exited {create_run.returncode}{note}\n"
                    f"stderr: {create_run.stderr.strip()}",
                )
            )
            return

        created = create_run.json()
        entity_id = find_id(created) if created is not None else None
        if entity_id is None:
            report.add(
                Result(
                    check.name,
                    Status.ERROR,
                    "created the entity but couldn't find its id in the response - "
                    "cannot verify the link's id or clean up automatically; raw response:\n"
                    f"{create_run.stdout.strip()}",
                )
            )
            return

        expected_fragment = check.expected_fragment(entity_id)
        verify_link(check.name, create_run, expected_fragment, report)
    finally:
        if entity_id and not keep:
            delete_run = run_cx(cx_bin, profile, check.delete_path + ["delete", entity_id, "--yes"])
            if delete_run.returncode != 0:
                print(
                    f"         WARNING: failed to clean up {check.name} {entity_id} - "
                    f"delete it manually. stderr: {delete_run.stderr.strip()}"
                )
        Path(tmp_path).unlink(missing_ok=True)


FIXTURE_INSTRUCTIONS = {
    "views": (
        "the saved-view definition schema isn't documented anywhere in this repo (skills/cx-observability-setup "
        "explicitly recommends templating rather than hand-authoring) - create one by hand in the console, then "
        "`cx views list -o json` to find its id, `cx views get <id> -o json > fixtures/view.json`, and edit the "
        "name so it's recognizable as a test object."
    ),
    "slos": (
        "the SLI/threshold definition schema isn't captured in the `Slo` struct in this repo - create one by hand, "
        "then `cx slos get <id> -o json > fixtures/slo.json` and give it a recognizable name."
    ),
    "suppression-rules": (
        "the `schedule` object's shape (recurrence/timezone) isn't documented in this repo - create one by hand, "
        "then `cx alerts suppression-rules get <id> -o json > fixtures/suppression_rule.json`."
    ),
    "notification-connectors": (
        "the connector `config` shape is type-specific and undocumented in this repo - create one by hand (e.g. a "
        "Slack or webhook connector), then "
        "`cx notifications connectors get <id> -o json > fixtures/connector.json`."
    ),
    "notification-routers": (
        "the `destinations`/matcher shape is undocumented in this repo - create one by hand, then "
        "`cx notifications routers get <id> -o json > fixtures/router.json`."
    ),
    "iam-groups": (
        "the `role`/`scope` sub-object shapes are undocumented in this repo - create one by hand, then "
        "`cx iam groups get <id> -o json > fixtures/group.json`."
    ),
}


# ---------------------------------------------------------------------------
# Cases - can't be created by the CLI (system-generated), so an existing
# case id must be passed in explicitly.
# ---------------------------------------------------------------------------


def run_case_checks(
    cx_bin: str,
    profile: Optional[str],
    case_id: Optional[str],
    case_mutate: bool,
    only: Optional[set],
    report: Report,
) -> None:
    if only and "cases" not in only:
        return
    if not case_id:
        report.add(
            Result(
                "cases",
                Status.SKIP,
                "cases are system-generated and cannot be created by `cx` - pass --case-id <id> "
                "(an existing case on your test team) to check the case console link.",
            )
        )
        return

    get_run = run_cx(cx_bin, profile, ["cases", "get", case_id, "-o", "json"])
    if get_run.returncode != 0:
        report.add(
            Result(
                "cases-get",
                Status.ERROR,
                f"`cx cases get {case_id}` exited {get_run.returncode}\nstderr: {get_run.stderr.strip()}",
            )
        )
        return
    expected = f"/#/cases?id={urllib.parse.quote_plus(case_id)}"
    verify_link("cases-get", get_run, expected, report)

    if not case_mutate:
        return

    # Priority override is the least destructive mutating case action
    # (purely cosmetic metadata, doesn't touch status/assignee/content), and
    # it's cleanly reversible with clear-priority - so it's the only
    # mutating case check enabled without extra flags. `--case-mutate`
    # still gates it since it does write to a real case.
    set_run = run_cx(
        cx_bin, profile, ["cases", "set-priority", case_id, "--priority", "P5", "--yes", "-o", "json"]
    )
    try:
        if set_run.returncode != 0:
            report.add(
                Result(
                    "cases-set-priority",
                    Status.ERROR,
                    f"`cx cases set-priority` exited {set_run.returncode}\nstderr: {set_run.stderr.strip()}",
                )
            )
            return
        verify_link("cases-set-priority", set_run, expected, report)
    finally:
        clear_run = run_cx(cx_bin, profile, ["cases", "clear-priority", case_id, "--yes"])
        if clear_run.returncode != 0:
            print(
                f"         WARNING: failed to clear-priority back on case {case_id} - "
                f"check it manually. stderr: {clear_run.stderr.strip()}"
            )


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__.split("\n\n")[0],
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--profile", help="cx profile to use (passed as `cx -p <profile>`). "
                         "Omit to use CX_API_KEY/CX_REGION env vars or the default ~/.cx profile.")
    parser.add_argument("--cx-bin", help="Path to the cx binary (default: `cx` on PATH, "
                         "then ./target/debug/cx, then ./target/release/cx).")
    parser.add_argument("--fixtures-dir", default=str(FIXTURES_DIR),
                         help=f"Directory containing entity creation fixtures (default: {FIXTURES_DIR}).")
    parser.add_argument("--case-id", help="An existing case id on your test team, to check "
                         "the case console link (cases can't be created by the CLI). Omit to skip case checks.")
    parser.add_argument("--case-mutate", action="store_true",
                         help="Also exercise a real mutating case command (set-priority, then "
                         "clear-priority to undo it) - requires --case-id. Off by default since it "
                         "writes to a real case, even though the roundtrip is fully reversible.")
    parser.add_argument("--include-olly", action="store_true",
                         help="Also check `cx olly ask`'s console link. Off by default - it triggers "
                         "a real AI-assistant call (cost + latency), unlike every other static check.")
    parser.add_argument("--only", help="Comma-separated list of check names to run "
                         "(see README.md for the full list). Default: run everything.")
    parser.add_argument("--keep", action="store_true",
                         help="Don't delete created entities afterwards (for debugging a failure "
                         "by hand). You are responsible for cleaning them up.")
    args = parser.parse_args()

    cx_bin = find_cx_bin(args.cx_bin)
    fixtures_dir = Path(args.fixtures_dir)
    only = set(args.only.split(",")) if args.only else None

    print(f"Using cx binary: {cx_bin}")
    version = run_cx(cx_bin, None, ["--version"])
    print(f"  {version.stdout.strip() or version.stderr.strip()}")
    if args.profile:
        print(f"Using profile: {args.profile}")
    print()

    report = Report()

    print("== Static (page-level) console links ==")
    run_static_checks(cx_bin, args.profile, only, report)
    run_olly_check(cx_bin, args.profile, args.include_olly, only, report)

    print("\n== Entity console links (create -> verify -> delete) ==")
    for check in ENTITY_CHECKS:
        run_entity_check(cx_bin, args.profile, check, fixtures_dir, only, args.keep, report)

    print("\n== Cases (requires an existing case id) ==")
    run_case_checks(cx_bin, args.profile, args.case_id, args.case_mutate, only, report)

    return report.summary_and_exit_code()


if __name__ == "__main__":
    sys.exit(main())
