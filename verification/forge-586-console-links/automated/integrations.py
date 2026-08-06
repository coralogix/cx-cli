"""
Automated replay for the `integrations` command group (PR176 console-link
verification).

Ground truth: OLD_DIR/results/integrations.jsonl (58 entries) plus
OLD_DIR/run_integrations.py / run_integrations2.py for the read-only part.

AUTOMATED here:
  - Pure read-only lookups against stable, pre-existing demo-team fixtures:
    `integrations list`, `integrations get slack-central`, `extensions
    list/get/deployed`, `contextual-data list/get/definition` (the latter
    two keyed on "StatusPage-Tracker", which -- like "Slack" for
    extensions -- is a built-in catalog key, not an instance this script
    creates; catalog lookups worked even though creating a real deployed
    instance never did).
  - Top-level `integrations create` / `test` / `update` / `delete`: every
    attempt in the original run failed with the exact same backend
    response, "API request failed (501): Unimplemented: Method Not
    Allowed", regardless of payload or id used. This is a pinned,
    documented backend limitation (not a PR176 defect, and not something
    reproducing it requires judgment about) -- replaying it is a safe,
    side-effect-free way to confirm the limitation is still in place. If a
    future run ever gets something other than 501 here, that is a
    meaningful signal (backend added support, or a real regression) and
    should be flagged for manual review.

NOT automated (see ../manual/integrations.md):
  - `integrations definition` / `template`: FAIL with an unexplained 404 in
    the original run: no note was recorded, so there's no pinned cause to
    mechanically compare against.
  - `integrations extensions deploy/update/undeploy`: 3 payload shapes were
    tried for deploy and none worked (404 Not Found each time); no
    known-good shape exists to replay.
  - `integrations contextual-data create` / `test`: schema could never be
    discovered (400 "unknown field" for every payload shape tried); `test`
    depends on a deployed instance that was never successfully created.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PAYLOADS_DIR = os.path.join(BASE_DIR, "payloads")
CREATE_PAYLOAD = os.path.join(PAYLOADS_DIR, "integrations_integration_create_v1.json")

GROUP = "integrations"
FORMATS = ["text", "json", "agents"]

KNOWN_501_NOTE = (
    "Known pre-existing backend limitation (pinned, not a PR176 defect): this "
    "endpoint consistently returns 'API request failed (501): Unimplemented: "
    "Method Not Allowed' on this backend regardless of payload/id used. See "
    "OLD_DIR/results/integrations.jsonl 'create'/'test'/'update (probe)'/"
    "'delete (probe)' entries for the original diagnosis. If this call ever "
    "returns something other than a 501 here, that's a real signal (backend "
    "added support, or a genuine regression) -- flag for manual review."
)


def _read_only_matrix():
    for fmt in FORMATS:
        r = run_cx(["integrations", "list"], output_format=fmt)
        record(GROUP, "list", fmt, r)

    for fmt in FORMATS:
        r = run_cx(["integrations", "get", "slack-central"], output_format=fmt)
        record(GROUP, "get", fmt, r)

    for fmt in FORMATS:
        r = run_cx(["integrations", "extensions", "list"], output_format=fmt)
        record(GROUP, "extensions list", fmt, r)

    for fmt in FORMATS:
        r = run_cx(
            ["integrations", "extensions", "get", "Slack"],
            output_format=fmt,
        )
        record(GROUP, "extensions get", fmt, r, notes="using pre-existing catalog id Slack")

    for fmt in FORMATS:
        r = run_cx(["integrations", "extensions", "deployed"], output_format=fmt)
        record(
            GROUP,
            "extensions deployed",
            fmt,
            r,
            notes="no id arg; lists deployed extensions, Slack is pre-existing deployed extension",
        )

    for fmt in FORMATS:
        r = run_cx(["integrations", "contextual-data", "list"], output_format=fmt)
        record(GROUP, "contextual-data list", fmt, r)

    for fmt in FORMATS:
        r = run_cx(
            ["integrations", "contextual-data", "get", "StatusPage-Tracker"],
            output_format=fmt,
        )
        record(GROUP, "contextual-data get", fmt, r)

    for fmt in FORMATS:
        r = run_cx(
            ["integrations", "contextual-data", "definition", "StatusPage-Tracker"],
            output_format=fmt,
        )
        record(GROUP, "contextual-data definition", fmt, r)


def _known_501_probes():
    # Top-level integrations create/test/update/delete: every attempt in the
    # original run hit the same 501 Unimplemented, no side effects possible.
    r = run_cx(
        ["integrations", "create", "--from-file", CREATE_PAYLOAD],
        output_format="json",
        extra_flags=["--yes"],
    )
    record(GROUP, "create", "n/a", r, notes=KNOWN_501_NOTE)

    for fmt in FORMATS:
        r = run_cx(
            ["integrations", "test", "--from-file", CREATE_PAYLOAD],
            output_format=fmt,
            extra_flags=["--yes"],
        )
        record(GROUP, "test", fmt, r, notes=KNOWN_501_NOTE)

    r = run_cx(
        [
            "integrations",
            "update",
            "cx-cli-pr176-probe-nonexistent",
            "--from-file",
            CREATE_PAYLOAD,
        ],
        output_format="json",
        extra_flags=["--yes"],
    )
    record(GROUP, "update (probe)", "n/a", r, notes=KNOWN_501_NOTE)

    r = run_cx(
        ["integrations", "delete", "nonexistent-test-id-cx-cli"],
        output_format="json",
        extra_flags=["--yes"],
    )
    record(GROUP, "delete (probe)", "n/a", r, notes=KNOWN_501_NOTE)


def run():
    _read_only_matrix()
    _known_501_probes()


if __name__ == "__main__":
    run()
