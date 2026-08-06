# webhooks -- manual verification items

## 1. `webhooks test` against the pre-existing production Slack webhook

**Exact command:**
```
cx -p kb-demo -o json --yes webhooks test 4436f664-1af3-41c0-a84a-61cc8c1b0868
```

**Why it needs judgment:** `4436f664-1af3-41c0-a84a-61cc8c1b0868` is a real,
pre-existing webhook configured on the `kb-demo` team (not something this
verification created), pointed at a real Slack channel. It was only invoked
once, deliberately, to confirm that the 501 seen when testing the
EMAIL_GROUP-type throwaway webhook is backend-wide rather than specific to
that webhook's type -- not as a routine regression check. Re-running it
unattended means firing a real Slack notification (or attempting to) against
a resource this verification doesn't own and can't safely pick a throwaway
substitute for. If the team's webhook list ever changes, this hardcoded id
may not exist at all.

**Decision criteria:** only re-run this by hand, after confirming with
whoever owns the `kb-demo` team that a Slack message landing in that channel
is acceptable, and re-check the id still exists first (`webhooks list`).

**Baseline (2026-08-03):** `FAIL`, exit 1, `API request failed (501):
Unimplemented: Method Not Allowed`. Notes: "fired real test event to the
pre-existing configured Slack webhook, to confirm the 501 above is
EMAIL_GROUP-type-specific rather than a general API/CLI issue" -- conclusion
was that it is a general issue (same 501 either way).

## 2. `webhooks actions create` (and the create/get/update/delete/batch/reorder
   chain that depends on it)

**Command shape:** unknown -- no valid `sourceType` enum value was ever
found.

**Why it needs judgment:** 17 distinct guesses for the `sourceType` field
(`0`, `1`, `2`, `LOGS`, `Logs`, `SOURCE_TYPE_LOGS`, `logs`, `LOG`, `Alert`,
`alert`, `Common`, `STANDARD`, `Standard`, `CUSTOM`, `Custom`, `GENERIC`,
`Generic`, `INTERNAL`, `External`, `WEBHOOK`, `Webhook`, `USER_DEFINED`,
`SYSTEM_DEFINED`) were all rejected by the proto validator as invalid enum
values. No action existed on the team beforehand to reverse-engineer a
working payload from either. Finding the right value needs either the
service's proto/OpenAPI spec or further live trial-and-error.

**Decision criteria:** find the actual accepted enum values (spec lookup, or
resume probing live and log every attempt via `record()`), confirm a create
succeeds and has a working delete route, then promote into
`automated/webhooks.py`. Until then, `get`/`update`/`delete`/`batch`/
`reorder` have nothing to test against and stay skipped too.

**Baseline (2026-08-03):** `SKIPPED` for all six (`create`, `get`, `update`,
`delete`, `batch`, `reorder`). Notes on `create`: "Could not determine valid
enum value for required field 'sourceType' after 17 probing attempts
(...) rejected by the proto validator as invalid enum values; no
source/docs reference consulted per task constraints (no reading src/).
Since no action could be created, actions get/update/delete/batch/reorder
are also skipped (no valid id to test against, and no existing actions
pre-existed on this team)."

## Not a manual item, but worth knowing

`webhooks update <id> --from-file ...` and `webhooks test <id> --yes`
(against the throwaway webhook this script creates) both `FAIL` every time
with a `501 Unimplemented`, pinned to a known backend limitation --
`automated/webhooks.py` replays both anyway since they're deterministic,
side-effect-free (the backend rejects before mutating/sending), and useful
as a live regression check. Only escalate if a rerun shows something other
than 501.

**Baseline (2026-08-03) for that comparison:**
- `webhooks update <id> --from-file webhook_update.json` (all 3 formats) --
  FAIL, exit 1: `API request failed (501): Unimplemented: Method No[t
  Allowed]`.
- `webhooks test <id> --yes` (all 3 formats) -- FAIL, exit 1: `API request
  failed (501): Unimplemented: Method Not Allowed`.
