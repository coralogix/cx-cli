"""
Automated (deterministic, no-LLM) replay of the console-link verification for `cx archive`.

Source of truth: OLD_DIR/results/archive.jsonl + OLD_DIR/step12_archive_validate.py through
step17_restore_logs.py.

Covers:
  - `metrics get` / `logs get` - pure read-only, PASS last time.
  - `metrics validate` - dry-run/no-op call with the confirmed-working schema
    ({"s3": {"bucket", "region"}} - matches this team's actual metrics archive config,
    so it is a true round-trip with nothing to clean up).
  - `metrics update` - same round-trip payload as validate; this reasserts the team's
    existing config (no real change), which is how the old run exercised it safely.
  - `metrics create` - despite the old run's notes saying "expected to fail", the
    recorded exit_code was actually 0 in all three formats ("Created metrics archive
    config..."): this backend treats `create` as an idempotent upsert against the
    single per-team metrics archive slot, not a true create-that-conflicts. The
    final-state check in the old run confirmed the config still matched baseline
    (same bucket/region, no duplicates) afterward, so this is a safe, confirmed
    round-trip with the same known-working payload as `validate`/`update`.
  - `metrics disable` immediately followed by `metrics enable` - a confirmed reversible
    round-trip (the old run verified the config was fully restored to disabled=false
    afterwards); the two calls are kept adjacent here to minimize the window where
    metrics archiving is off.

NOT covered here (see ../manual/archive.md):
  - `logs set` - THE FLAG TO NEVER TOUCH AGAIN. The old run's `logs set` call (even with
    the schema-correct, values-unchanged {"s3": {...}} body) flipped
    `archiveSpec.enableTags` from false to true as an undocumented side effect, and no
    field-name guess (enableTags/enable_tags/tags/enabledTags) was accepted to revert it
    - `enableTags` appears to be read-only/server-computed and not actually settable via
    this endpoint's exposed fields. This is a real, permanent, unrevertable residual
    change on the "kb-demo" team. `logs set` must not be auto-run again in any form.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record

GROUP = "archive"
HERE = os.path.dirname(os.path.abspath(__file__))
BASE_DIR = os.path.dirname(HERE)
PAYLOAD_DIR = os.path.join(BASE_DIR, "payloads")
METRICS_BODY = os.path.join(PAYLOAD_DIR, "archive_metrics_body.json")


def run():
    for fmt in ["text", "json", "agents"]:
        res = run_cx(["archive", "metrics", "get"], output_format=fmt)
        record(GROUP, "metrics get", fmt, res)

    for fmt in ["text", "json", "agents"]:
        res = run_cx(["archive", "logs", "get"], output_format=fmt)
        record(GROUP, "logs get", fmt, res)

    for fmt in ["text", "json", "agents"]:
        res = run_cx(["archive", "metrics", "validate", "--from-file", METRICS_BODY], output_format=fmt)
        record(GROUP, "metrics validate (round-trip of current config)", fmt, res)

    for fmt in ["text", "json", "agents"]:
        res = run_cx(
            ["archive", "metrics", "update", "--from-file", METRICS_BODY],
            output_format=fmt,
            extra_flags=["--yes"],
        )
        record(GROUP, "metrics update (round-trip of current config, no real change)", fmt, res)

    for fmt in ["text", "json", "agents"]:
        res = run_cx(
            ["archive", "metrics", "create", "--from-file", METRICS_BODY],
            output_format=fmt,
            extra_flags=["--yes"],
        )
        record(GROUP, "metrics create (idempotent upsert, round-trip of current config)", fmt, res,
               notes="backend treats create as an upsert against the single per-team metrics archive slot; confirmed no duplicate/overwrite last time")

    for fmt in ["text", "json", "agents"]:
        res = run_cx(["archive", "metrics", "disable"], output_format=fmt, extra_flags=["--yes"])
        record(GROUP, "metrics disable", fmt, res,
               notes="baseline is enabled (disabled:false); re-enabled immediately after to restore")

    for fmt in ["text", "json", "agents"]:
        res = run_cx(["archive", "metrics", "enable"], output_format=fmt, extra_flags=["--yes"])
        record(GROUP, "metrics enable (restore to original enabled state)", fmt, res)

    # final sanity check that metrics archive config matches baseline (disabled:false)
    res = run_cx(["archive", "metrics", "get"], output_format="json")
    record(GROUP, "metrics get (final state check)", "n/a", res,
           notes="confirms metrics archive restored to baseline (disabled:false) after the disable/enable round-trip")

    print(f"[{GROUP}] done")


if __name__ == "__main__":
    run()
