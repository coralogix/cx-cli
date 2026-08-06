# iam -- manual items

`iam` was the biggest and messiest group (129 JSONL entries). See
`../automated/iam.py`'s module docstring for the full replayed/not-replayed
split. This file covers everything **not** replayed automatically.

## `iam scopes create` / `update` / `get` / `delete`

**Command (best-effort, from the original session):**

```
cx -p kb-demo -o <text|json|agents> --yes iam scopes create --from-file scope_final.json
```

**Why it needs judgment:** 16 schema-discovery attempts never produced a
working payload. `display_name` (not `name`) and proto-enum-style
`filters[].entity_type` (`ENTITY_TYPE_LOGS`, not `logs`/`LOGS`) were
confirmed required, but `filters[].expression` is accepted as a string by
the proto validator yet the platform's expression compiler always reports
`Expression: []` regardless of content -- even using the API's own example
string verbatim. `update`/`get`/`delete` were never exercised because no
scope was ever successfully created to act on.

**Decision criteria for a future pass:**
- If someone finds the exact expression-field shape (via API docs, the
  Coralogix web console's scope builder network requests, or
  `src/commands/iam/api.rs`) and gets a real 200 with a `consoleUrl`,
  promote `create` to `automated/iam.py` using that payload, then get/update/
  delete follow naturally.
- If it still fails with the same `Failed to compile expression. Expression: []`
  signature, that's the same known issue.
- Any *different* error is worth investigating before concluding anything.

**Known baseline** (`OLD_DIR/results/iam.jsonl`, `scopes create`, status
SKIPPED, all 3 formats):

> Could not construct a valid scope payload after 16 schema-discovery
> attempts... This looks like a pre-existing schema-discoverability/
> documentation gap in the underlying Coralogix scopes API, not a PR176
> regression... Not a cx CLI bug -- the CLI faithfully forwards --from-file
> JSON while API errors indicate a schema mismatch.

stderr: `API request failed (412): Failed Precondition: Failed to compile
expression. Expression: [], Errors: [expected version tag. e.g. ...`

---

## `iam groups users <group_id>`

**Command:**

```
cx -p kb-demo -o <text|json|agents> iam groups users <group_id>
```

**Why it needs judgment:** every group id tried -- three freshly-created
empty test groups (147422/147423/147424) *and* a real pre-existing group
("Users", id 141711) -- returned a 404. That's suspicious for a
pre-existing real group; either the endpoint needs a different id shape, a
different permission scope, or there's a real bug. Not something to
mechanically retry without a human looking at the actual API contract.

**Decision criteria for a future pass:**
- Try it against a group that is known (via the live console UI) to have
  at least one member, to rule out "the API 404s for genuinely-empty/
  zero-member groups" as an explanation.
- If it 404s even against a group with confirmed members, that's a real bug
  worth filing/fixing, not a testing artifact.
- If it now succeeds, promote to `automated/iam.py` using a stable,
  known-good group id.

**Known baseline** (`OLD_DIR/results/iam.jsonl`, `groups users`, status
FAIL, exit 1, all attempts):

> re-tested against pre-existing 'Users' group (141711) after 404 on empty
> fresh test groups; read-only, no console link expected

stderr: `API request failed (404): Not Found: Not Found`

---

## `iam api-keys list`

**Command:**

```
cx -p kb-demo -o <text|json|agents> iam api-keys list
```

**Why it needs judgment, not automation:** this is very likely **already
fixed**, not an open question -- but automating it now would encode the old,
known-broken behavior as the "expected" baseline, so it needs one fresh
manual comparison first. Recent history on this branch (see `git log`)
includes `fix(iam): point api-keys list at the working bare endpoint, drop
admin list` (commit `cc496b5`). Reading the current source
(`src/commands/api_keys/api.rs::list()`) confirms it now calls
`API_KEYS_BASE` directly with no `key_id` param at all -- the exact class of
bug described below no longer looks reachable from this code path.

**Decision criteria for a future pass:**
- Run it once by hand. If it now returns a real key list (and a
  `consoleUrl`), that confirms the fix -- promote to `automated/iam.py`.
- If it still fails with the same `key_id invalid character` signature,
  that's a genuine regression/reintroduction worth flagging immediately
  (this exact class of bug was supposedly already fixed on this branch).

**Known baseline** (`OLD_DIR/results/iam.jsonl`, `api-keys list`, status
FAIL, exit 1, all 3 formats, **predates** commit `cc496b5`):

> FAIL: 'key_id invalid character: found `l` at 1' -- looks like the CLI is
> treating the literal subcommand token or an empty/placeholder key_id
> incorrectly for this endpoint on this demo team.

stderr: `API request failed (400): Bad Request: key_id invalid character:
found \`l\` at 1`

---

## `iam api-keys admin set-status`

**Command:**

```
cx -p kb-demo -o <text|json|agents> --yes iam api-keys admin set-status --ids <key_id>          # deactivate
cx -p kb-demo -o <text|json|agents> --yes iam api-keys admin set-status --ids <key_id> --active  # activate
```

(`--active` is a bare boolean flag -- `clap` rejects `--active false`/`--active true`
with "unexpected argument 'false' found"; omit the flag to deactivate, pass
it bare to activate.)

**Why this needs judgment, and is likely a real, still-present bug:**
confirmed independently by reading current source, not just the old JSONL --
`src/commands/api_keys/mod.rs::run_admin_set_status` (around line 396) sends
`{"keyIds": ids, "isActive": active}` to `POST {API_KEYS_BASE}/all/status`.
The sibling `run_admin_delete` sends the *same* `{"keyIds": ids}` shape to
`POST {API_KEYS_BASE}/all/delete` and that one works (confirmed PASS in the
old run). So the `/all/status` endpoint's proto schema apparently expects a
different field name than `/all/delete`'s -- this is a genuine backend/CLI
field-name mismatch on the `set-status` path specifically, not a flag-usage
mistake, and it has never once succeeded across any attempt in the old run.

**Decision criteria for a future pass:**
- This looks worth fixing rather than just re-verifying: try alternate field
  names for the request body (e.g. `ids` instead of `keyIds`, or check
  whatever the console UI's own network request to this endpoint sends) in
  `src/commands/api_keys/mod.rs`.
- Once a shape gets a real 200, promote to `automated/iam.py`.
- If not fixed, re-running will keep reproducing the exact same
  `unknown field "keyIds"` error -- that alone doesn't need re-diagnosis
  each time, just confirmation nothing changed.

**Known baseline** (`OLD_DIR/results/iam.jsonl`, `api-keys admin set-status`,
status FAIL, exit 1, all attempts after fixing the `--active false` usage
error):

> ✅ per coverage table; --active is a boolean flag (omitted = deactivate);
> check for consoleUrl

stderr: `API request failed (400): Bad Request: proto: (line 1:2): unknown
field "keyIds"`

---

## `iam users create` / `update` / `set-status`

**Command (best-effort, from the original session):**

```
cx -p kb-demo -o <text|json|agents> --yes iam users create --from-file user_create_try1.json
```

**Why it needs judgment:** `create` never got a payload past the proto-JSON
validator after ~16 attempts (tried `name`/`email`/`user_name`/`user_email`/
`userName`/`roleIds`/`login`/`identifier`/`user`/`invite`, both as a bare
array and as a `{"users": [...]}` wrapper -- all rejected as "unknown
field"). Critically, **a successful `create` sends a real invite email** to
whatever address is in the payload -- so this must never be blindly
auto-replayed even after a working shape is found; a real run always sends
a real invite to a real inbox. `update`/`set-status` were never exercised
because no disposable test user was ever created, and testing those against
a real teammate's account would be a real, disruptive, unwanted side effect.

**Decision criteria for a future pass:**
- If someone finds the required payload shape (API docs, or the console
  UI's own "invite user" network request), that unblocks `create` -- but
  any automated replay must target a throwaway/dedicated test mailbox the
  team controls, never a random or existing address, and the fact that it
  emails a real inbox should probably keep this in MANUAL permanently
  rather than promoting it to `automated/iam.py`.
- `update`/`set-status` stay manual indefinitely unless a disposable test
  user account becomes available to safely mutate.

**Known baseline** (`OLD_DIR/results/iam.jsonl`, `users create`, status
SKIPPED, all 3 formats):

> Could not construct a valid users/create payload after ~16
> schema-discovery attempts... Given users/create sends a REAL invite email
> on success, and we never got a payload past validation, NO invite email
> was sent... This is a schema-discoverability gap, not confirmed to be a
> PR176 regression, since no successful create response was ever obtained
> to inspect for consoleUrl.

stderr: `API request failed (400): Bad Request: json: cannot unmarshal
object into Go value of type []json.RawMessage`

---

## Informational only -- not an open item

**`iam api-keys admin list`** existed in the original run (and PASSED, with
a `consoleUrl`), but per commit `cc496b5` ("point api-keys list at the
working bare endpoint, drop admin list") it has since been **removed
entirely** -- confirmed absent from the current `ApiKeysAdminCmd` enum in
`src/main.rs`. Its old read-only coverage is superseded by the fixed plain
`iam api-keys list` (see above). Nothing to do here; not a gap.
