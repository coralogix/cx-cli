"""
Automated replay for the `ai-center` command group (PR176 console-link
verification).

Ground truth: OLD_DIR/results/ai-center.jsonl (51 entries) plus
OLD_DIR/step2_ai_center.py .. step5_ai_center_cleanup.py for sequencing.

AUTOMATED here:
  - Pure read-only lookups against stable team/demo fixtures: applications
    list/get, coverage, model-pricing get, evaluations list, custom-
    evaluations list, custom-evaluations list-for-application (against the
    pre-existing "otel-demo/guardrails-demo" app, id
    c59d5dc2-9095-4feb-8607-1acf37a9b799, and the "Unknown/Unknown" app id
    0eb7fcf0-6b68-48a0-9bfa-73636074c463).
  - evaluations create -> get -> list -> update -> delete: full lifecycle
    with a known-working payload shape (eval_body.json / eval_patch.json)
    and a working delete route. Uses a fresh subsystem name each run so
    reruns never collide, and cleans up via `evaluations delete`.
  - model-pricing set -> restore: known-working payload, and this is a
    reversible team-wide setting (not an object needing its own delete
    route) -- the script captures whatever pricing existed *before* it
    mutated anything and restores exactly that, rather than assuming the
    team started out empty.

NOT automated (see ../manual/ai-center.md):
  - Everything under `custom-evaluations` that CREATES a policy (create,
    update, add, remove, the post-add list-for-application check). The
    original session found there is NO delete route for a custom-evaluation
    policy object once created (only add/remove, which attach/detach from
    an application) -- every create leaves a permanent orphan in the team.
    Replaying that lifecycle here would accumulate a new orphan every run,
    so it is deliberately left to manual judgment.
"""

import json
import os
import sys
import uuid

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PAYLOADS_DIR = os.path.join(BASE_DIR, "payloads")
EVAL_BODY_TEMPLATE = os.path.join(PAYLOADS_DIR, "ai-center_eval_body.json")
EVAL_PATCH_PATH = os.path.join(PAYLOADS_DIR, "ai-center_eval_patch.json")
PRICE_BODY_PATH = os.path.join(PAYLOADS_DIR, "ai-center_price_body.json")

GROUP = "ai-center"
FORMATS = ["text", "json", "agents"]

# Stable, pre-existing demo-team fixtures (not created by this script).
APP_ID = "0eb7fcf0-6b68-48a0-9bfa-73636074c463"  # "Unknown"/"Unknown"
APP2_ID = "c59d5dc2-9095-4feb-8607-1acf37a9b799"  # otel-demo / guardrails-demo


def _extract(stdout, envelope_key=None):
    try:
        data = json.loads(stdout)
        obj = data[0] if isinstance(data, list) else data
        if envelope_key and isinstance(obj, dict) and envelope_key in obj:
            obj = obj[envelope_key]
        return obj
    except Exception:
        return None


def _make_eval_payload(suffix):
    with open(EVAL_BODY_TEMPLATE) as f:
        body = json.load(f)
    body["subsystem"] = f"guardrails-demo-pr176-{suffix}"
    tmp_path = os.path.join(PAYLOADS_DIR, f"_run_ai-center_eval_{suffix}.json")
    with open(tmp_path, "w") as f:
        json.dump(body, f)
    return tmp_path


def _read_only_matrix():
    for fmt in FORMATS:
        r = run_cx(["ai-center", "applications", "list"], output_format=fmt)
        record(GROUP, "applications list", fmt, r)

    for fmt in FORMATS:
        r = run_cx(["ai-center", "coverage"], output_format=fmt)
        record(GROUP, "coverage", fmt, r)

    for fmt in FORMATS:
        r = run_cx(["ai-center", "model-pricing", "get"], output_format=fmt)
        record(GROUP, "model-pricing get", fmt, r)

    for fmt in FORMATS:
        r = run_cx(["ai-center", "evaluations", "list"], output_format=fmt)
        record(GROUP, "evaluations list (unscoped)", fmt, r)

    for fmt in FORMATS:
        r = run_cx(["ai-center", "custom-evaluations", "list"], output_format=fmt)
        record(GROUP, "custom-evaluations list", fmt, r)

    for fmt in FORMATS:
        r = run_cx(["ai-center", "applications", "get", APP_ID], output_format=fmt)
        record(GROUP, "applications get", fmt, r)

    for fmt in FORMATS:
        r = run_cx(
            ["ai-center", "custom-evaluations", "list-for-application", APP2_ID],
            output_format=fmt,
        )
        record(GROUP, "custom-evaluations list-for-application (before add)", fmt, r)


def _evaluations_lifecycle():
    suffix = uuid.uuid4().hex[:8]
    tmp_payload = _make_eval_payload(suffix)
    eval_id = None
    try:
        r = run_cx(
            ["ai-center", "evaluations", "create", "--from-file", tmp_payload],
            output_format="json",
            extra_flags=["--yes"],
        )
        record(GROUP, "evaluations create (setup)", "n/a", r)
        if r["exit_code"] == 0:
            obj = _extract(r["stdout"], envelope_key="aiEvaluation")
            eval_id = obj.get("id") if obj else None

        if not eval_id:
            print("ai-center._evaluations_lifecycle(): create did not return a usable id -- skipping rest")
            return

        for fmt in FORMATS:
            r = run_cx(["ai-center", "evaluations", "get", eval_id], output_format=fmt)
            record(GROUP, "evaluations get", fmt, r)

        for fmt in FORMATS:
            r = run_cx(["ai-center", "evaluations", "list"], output_format=fmt)
            record(GROUP, "evaluations list (populated)", fmt, r)

        for fmt in FORMATS:
            r = run_cx(
                ["ai-center", "evaluations", "update", eval_id, "--from-file", EVAL_PATCH_PATH],
                output_format=fmt,
                extra_flags=["--yes"],
            )
            record(GROUP, "evaluations update", fmt, r)
    finally:
        if eval_id:
            r = run_cx(
                ["ai-center", "evaluations", "delete", eval_id],
                output_format="json",
                extra_flags=["--yes"],
            )
            record(
                GROUP,
                "evaluations delete (cleanup)",
                "n/a",
                r,
                notes="cleanup call; a non-zero exit here (e.g. target already gone) is tolerated as non-fatal",
            )
        try:
            os.remove(tmp_payload)
        except OSError:
            pass


def _model_pricing_lifecycle():
    # Capture whatever pricing exists *before* we touch anything, so restore
    # is exact regardless of what the team's current state happens to be.
    r = run_cx(["ai-center", "model-pricing", "get"], output_format="json")
    original_prices = {}
    obj = _extract(r["stdout"], envelope_key="pricing")
    if isinstance(obj, dict) and isinstance(obj.get("prices"), dict):
        original_prices = obj["prices"]

    for fmt in FORMATS:
        r = run_cx(
            ["ai-center", "model-pricing", "set", "--from-file", PRICE_BODY_PATH],
            output_format=fmt,
            extra_flags=["--yes"],
        )
        record(
            GROUP,
            "model-pricing set",
            fmt,
            r,
            notes="Sets a temporary override for gpt-4o; restored to pre-run state immediately after.",
        )

    restore_path = os.path.join(PAYLOADS_DIR, f"_run_ai-center_price_restore_{uuid.uuid4().hex[:8]}.json")
    try:
        with open(restore_path, "w") as f:
            json.dump(original_prices, f)
        r = run_cx(
            ["ai-center", "model-pricing", "set", "--from-file", restore_path],
            output_format="json",
            extra_flags=["--yes"],
        )
        record(
            GROUP,
            "model-pricing set (restore to pre-run state)",
            "n/a",
            r,
            notes=f"Restored to the pricing captured before this run started: {original_prices!r}",
        )
    finally:
        try:
            os.remove(restore_path)
        except OSError:
            pass


def run():
    _read_only_matrix()
    _evaluations_lifecycle()
    _model_pricing_lifecycle()


if __name__ == "__main__":
    run()
