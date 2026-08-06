# recording-rules - manual verification items

## `recording-rules create` / `get` / `update` / `delete`

**Command** (the corrected schema discovered last time - `create` returns exit 0 with
this body but see below):

```
cx -p kb-demo -o json --yes recording-rules create --from-file body.json
# body.json: {"groups": [{"name": "cx-cli-pr176-test-recording-rule", "interval": 60,
#             "rules": [{"record": "cx_cli_pr176_test_metric", "expr": "vector(1)"}]}]}
```

**Why it needs judgment:** the first `create` attempt (bare `{"name","interval":"60s",
"rules":[...]}`, matching the GET/LIST group shape) FAILed - the API rejected `interval`/
`rules` as unknown top-level fields. The corrected schema above returns exit 0 / HTTP 200,
but the created group **never appears** in a subsequent `list` or is gettable by any ID,
even after a 25s wait. `get`, `update`, and `delete` were all SKIPPED because there was no
ID to test them with. This means:
1. We cannot confirm whether `create` actually persists anything server-side (if it does,
   it's an invisible orphan we have no way to find or clean up via the CLI).
2. There is no confirmed working delete route for whatever `create` may or may not have
   left behind.

Both of those are exactly the conditions that must not be mechanically automated.

**Decision criteria:**
- Re-run the corrected-schema `create` call, then `list` (with a short wait, e.g. 5-30s,
  and try a couple of retries) to see if the group is now visible.
  - Still invisible -> same known gap; leave as manual. If you suspect an invisible
    orphan is accumulating on the live team from repeated manual runs, escalate to
    check the backend/account directly (not via this CLI) rather than guessing.
  - Now visible with an ID -> real fix/change on the backend side; exercise `get`,
    `update` (any body change), and `delete`, confirm delete actually removes it via a
    follow-up `list`, and only then promote the full lifecycle into
    `automated/recording-rules.py`.

**Known baseline** (from `OLD_DIR/results/recording-rules.jsonl`):
- `create (setup)` (first attempt, wrong schema): `FAIL`
- `create (setup, corrected schema...)`: `PASS` (exit 0) but notes: "list command itself
  works (exit 0) but returns 0 groups even though 'create' above returned exit 0/200 for
  the same account moments earlier - the created group never appears via list or get in
  this environment, so get/update/delete (and their console links) could not be
  exercised."
- `get`: `SKIPPED` - "No recording rule group ID available: 'create' returns 200/exit-0
  but the submitted group never appears in 'list' or is gettable by any ID in this demo
  team, even after a 25s wait."
- `update`: `SKIPPED` - "Same blocker as 'get'."
- `delete (cleanup)`: `SKIPPED` - "No group was ever visible via list/get to delete;
  nothing to clean up (the create call reported success but no resource is
  retrievable/cleanable)."
