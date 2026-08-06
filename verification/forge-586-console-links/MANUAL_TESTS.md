# Manual verification items -- PR #176 console links (FORGE-586)

Everything here needs a human or LLM to look at fresh output and make a
judgment call before it can be trusted -- schema discovery against
undocumented APIs, deciding whether a FAIL is a known pre-existing issue or
a new regression, or anything that would mutate real production data
(support cases, a real teammate account) or an irreversible setting.

Each entry comes out of a one-time, 740-test manual verification run
against a real Coralogix team (profile `kb-demo`) that exercised every
console-link-eligible subcommand of the `cx` CLI. See `README.md` for the
full picture (what's automated vs. manual and why) and
`automated/<group>.py` for the mechanically-safe counterpart of each group.

## Groups

- [ai-center](#ai-center) -- 5 item(s)
- [alerts](#alerts) -- 0 item(s)
- [archive](#archive) -- 2 item(s)
- [cases](#cases) -- 7 item(s)
- [cleanup](#cleanup) -- 1 item(s)
- [completions](#completions) -- 2 item(s)
- [dashboards](#dashboards) -- 2 item(s)
- [dataprime](#dataprime) -- 1 item(s)
- [docs](#docs) -- 2 item(s)
- [e2m](#e2m) -- 1 item(s)
- [enrichments](#enrichments) -- 2 item(s)
- [iam](#iam) -- 6 item(s)
- [integrations](#integrations) -- 5 item(s)
- [logs](#logs) -- 1 item(s)
- [metrics](#metrics) -- 1 item(s)
- [notifications](#notifications) -- 3 item(s)
- [olly](#olly) -- 1 item(s)
- [parsing-rules](#parsing-rules) -- 1 item(s)
- [profiles](#profiles) -- 1 item(s)
- [recording-rules](#recording-rules) -- 1 item(s)
- [retentions](#retentions) -- 2 item(s)
- [schema](#schema) -- 1 item(s)
- [search-fields](#search-fields) -- 1 item(s)
- [slos](#slos) -- 2 item(s)
- [spans](#spans) -- 1 item(s)
- [suppression-rules](#suppression-rules) -- 2 item(s)
- [tco](#tco) -- 3 item(s)
- [usage](#usage) -- 0 item(s)
- [views](#views) -- 3 item(s)
- [webhooks](#webhooks) -- 3 item(s)

---

<a id="ai-center"></a>

# ai-center -- manual items

All of these live under `ai-center custom-evaluations`. The root problem,
per the original session's summary and `step5_ai_center_cleanup.py`: **there
is no delete route for a custom-evaluation policy object once created** --
only `add` (attach to an application) and `remove` (detach) exist. Every
`create` therefore leaves a permanent orphan in the team. Automating any
part of this lifecycle would accumulate a new orphan on every rerun forever,
which fails the "known, working delete route" requirement for AUTOMATED
outright. (Contrast with `ai-center evaluations`, which has a real `delete`
and IS automated in `../automated/ai-center.py`.)

## `ai-center custom-evaluations create --from-file ce_body.json`

```json
{"name": "PR176 Test Custom Eval", "instructions": "Flag any mention of the word banana in the response.", "policyType": "QUALITY"}
```

```
cx -p kb-demo -o json --yes ai-center custom-evaluations create --from-file ce_body.json
```

**Why manual:** creates a policy object with no delete route.
**Decision criteria:** only run this if you're prepared to leave the
resulting policy in the team permanently (or if/when a real delete route
ships -- check `skills/cx-ai-center/SKILL.md` and `src/commands/ai-center/
api.rs` first). **Baseline:** PASS, exit 0, created policy id
`bda30d23-cdd6-4bae-ad16-ef622a8f788d`, response includes `consoleUrl`
pointing at `.../ai-center/overview/eval-catalog`.

## `ai-center custom-evaluations update <id> --from-file ce_update.json`

```json
{"name": "PR176 Test Custom Eval (updated)"}
```

**Why manual:** only meaningful against a policy created above (same orphan
concern by association). **Baseline:** PASS across all 3 formats.

## `ai-center custom-evaluations add <policy-id> <app-id>` / `remove <policy-id> <app-id>`

```
cx -p kb-demo -o <fmt> --yes ai-center custom-evaluations add <policy-id> c59d5dc2-9095-4feb-8607-1acf37a9b799
cx -p kb-demo -o <fmt> --yes ai-center custom-evaluations remove <policy-id> c59d5dc2-9095-4feb-8607-1acf37a9b799
```

**Why manual:** these are one-shot state transitions (attach/detach), and
they only make sense against a policy from the `create` step above, so they
inherit the same "leaves an orphan behind" concern -- there's no clean way
to test add/remove in isolation without first creating (and never being
able to delete) a policy.

**Known baseline, including an already-diagnosed FAIL pattern** (not a
regression if you see it again): `add`/`remove` each PASSed on the first
(`text`) format call, then the `json`/`agents` re-runs of the *same*
id-pair correctly got `409 Already-Exists` (add) / `404 Not-Linked`
(remove) -- expected idempotency-guard behavior when the exact same
mutation is replayed 3x in a row across formats, not a CLI/PR176 defect.
Quoting the original annotation verbatim:

> add/remove are one-shot state transitions (attach/detach), so testing the
> same id-pair across 3 output formats in a loop necessarily re-runs the same
> mutation twice more. The first ('text') call succeeded each time (confirmed
> working, with consoleUrl present); the 'json'/'agents' re-runs correctly got
> API 409 Already-Exists / 404 Not-Linked, proving the backend's idempotency
> guard works, not a CLI/PR176 defect. Final state: policy detached from the
> app (clean).

**Decision criteria:** if a future manual run sees the *first* format-call
fail (not just the repeat calls), that's a real regression worth
investigating; the repeat-call 409/404 pattern itself is expected and not
worth re-litigating.

## `ai-center custom-evaluations list-for-application <app-id>` (after add)

```
cx -p kb-demo -o json ai-center custom-evaluations list-for-application c59d5dc2-9095-4feb-8607-1acf37a9b799
```

**Why manual:** only meaningful as a checkpoint inside the create/add/remove
lifecycle above (confirms the attach took effect); the "before add" call
against the same app id, which doesn't depend on our own mutation, IS
automated. **Baseline:** PASS.

## `ai-center custom-evaluations delete` -- does not exist

**Baseline:** SKIPPED, with the note:

> No delete route exists for custom-evaluation policies by design (per
> skills/cx-ai-center/SKILL.md) -- only add/remove (attach/detach) are
> supported. The test policy 'PR176 Test Custom Eval (updated)'
> (bda30d23-cdd6-4bae-ad16-ef622a8f788d) was detached from all applications
> via `remove` but the policy object itself remains in the team (cannot be
> deleted via cx).

**Decision criteria:** re-check `skills/cx-ai-center/SKILL.md` and the API
client periodically -- if a delete route is ever added, this whole group
(create/update/add/remove/delete) can be promoted to
`../automated/ai-center.py` as a clean, self-cleaning lifecycle.

---

<a id="alerts"></a>

# alerts -- manual items

None. Every subcommand exercised in the original session
(`OLD_DIR/results/alerts.jsonl`, 20 entries) passed cleanly on the first try
with a known-working payload and lifecycle: `create` (setup) -> `get` /
`enable` / `disable` / `list` / `events` / `event-stats` across
text/json/agents -> `delete` (cleanup). There were no FAILs, no ambiguous
output, and no irreversible or judgment-laden steps.

All of it is replayed mechanically by `../automated/alerts.py`.

---

<a id="archive"></a>

# archive - manual verification items

## `archive logs set` (IRREVERSIBLE - already flipped on kb-demo)

**Command** (the "corrected schema" that the old run considered a safe round-trip, and
which is exactly what caused the residual side effect):

```
cx -p kb-demo -o <text|json|agents> --yes archive logs set --from-file body.json
# body.json: {"s3": {"bucket": "olly-knowledge-base-otel", "region": "eu-north-1"}}
```

**Why it needs judgment:** the logs archive target's `get` response includes an
`archiveSpec` object (`archivingFormatId`, `isActive`, `enableTags`), but `set` rejects
`archiveSpec` as an unknown field - only `s3` is accepted. The old run's very first
schema-correct `set` call (bucket/region unchanged, matching baseline) had the
undocumented side effect of flipping `archiveSpec.enableTags` from `false` to `true`.
Every field-name guess tried afterward to restore it (`enableTags`, `enable_tags`,
`tags`, `enabledTags`, alone and combined with `isActive`/`archivingFormatId`) was
rejected as an "unknown field" - `enableTags` appears to be read-only/server-computed and
not actually settable through this endpoint at all. **This must never be auto-replayed**:
even though the bucket/region round-trip itself looks inert, `set`'s hidden side effect
on `enableTags` is not, and there is no way back once it flips.

**Decision criteria:**
- Do not run `archive logs set` in any form against any team without explicit human
  sign-off, since its true side effects on fields outside the request body (like
  `enableTags`) are not fully understood.
- If investigating this area, use `archive logs get` (read-only, safe, in
  `automated/archive.py`) to inspect current state.
- If you need to determine whether `enableTags` is truly unsettable (vs. just needing a
  different field name/nesting not yet tried), that discovery work belongs here, not in
  the automated script - and any successful revert should be double-checked before ever
  being folded into automation.

**Known baseline** (from `OLD_DIR/results/archive.jsonl` and `step15`-`step17` scripts):
- `logs set (attempt 1: archiveSpec unknown field)`: `FAIL` - "Schema discovery:
  archiveSpec (present in get response) is not accepted on set -- only s3."
- `logs set (correct schema, round-trip of current config)`: `PASS` (all formats) - but
  this is the call that flipped `enableTags` false -> true as a side effect.
- `logs get (final state check)`: `PASS` - notes: "bucket/region unchanged
  (olly-knowledge-base-otel / eu-north-1); archiveSpec.isActive restored to true (matches
  baseline). archiveSpec.enableTags is now true, was false at baseline -- could NOT
  restore this specific sub-field: every field-name guess ... was rejected as 'unknown
  field' by `archive logs set` ... enableTags=true is the one field left in a different
  state than found, and it could not be reverted via any exposed field name."
- Current state on kb-demo going forward: `archiveSpec.enableTags=true` (confirmed by
  `archive logs get` immediately after, and expected to still read `true` on any fresh
  `automated/archive.py` run's `logs get` call).

## Note on `archive metrics create` (NOT manual - resolved, now automated)

The old jsonl's notes on `metrics create` say "expected to fail... confirms no accidental
duplicate/overwrite," which reads like a FAIL, but the recorded `exit_code` for all three
formats was actually `0` ("Created metrics archive config in profile 'kb-demo'."). This
backend evidently treats `archive metrics create` as an idempotent upsert against the
single per-team metrics archive slot rather than a true create-that-conflicts, and the
old run's final-state check confirmed the config still matched baseline (same
bucket/region, no duplicate configs) afterward. Since it's a genuine PASS with a known,
safe, reversible payload (identical to `metrics validate`/`metrics update`), it has been
included in `automated/archive.py` rather than left here - flagging this in case a future
reader also gets misled by the "expected to fail" note text without checking the raw
`exit_code`.

---

<a id="cases"></a>

# cases -- manual items

`cases` has no create/list-your-own-object of its own. Every subcommand in
the original session (`OLD_DIR/results/cases.jsonl`, 42 entries) ran against
3 REAL, pre-existing demo-team cases:

- `8623c2d0-2d1b-5094-a3b6-d454c112e5d9` (became CASE-43)
- `b0ea9128-e734-58a3-a160-cf46e357a9ed` (became CASE-96)
- `ee1dca07-c329-5250-917f-3b0374f86af0` (became CASE-97)

All 3 were resolved and then **closed** during that run ("per explicit
approval to run everything" -- a one-time, human-approved decision for that
session, not a standing license). There is no undo for CLOSED status. Per
the task's classification rules, ANY subcommand that mutates a real case's
state must be MANUAL -- never auto-mutate real production cases on a
schedule with no throwaway target to pick. Only the 4 genuinely read-only
lookups (`get`, `events list`, `events get`, `notifications`) are in
`../automated/cases.py`.

Every item below passed cleanly last time (status PASS, exit 0, empty
`notes` unless stated) -- the issue isn't reliability, it's that mutating a
real case is inherently a judgment call, every single time.

## `cases update <id> --title ...`

```
cx -p kb-demo -o <fmt> --yes cases update <case-id> --title "<new title>"
```

**Why manual:** overwrites the real title of a real case. Baseline: PASS,
e.g. `cases update 8623c2d0-... --title "[otel-demo] Error Log Rate - PR176 smoke test"`.
**Criteria:** only run this against a case you specifically intend to
relabel; never on an automated schedule.

## `cases comment <id> --text ...`

```
cx -p kb-demo -o <fmt> --yes cases comment <case-id> --text "PR176 console-link smoke test comment"
```

**Why manual:** appends a permanent, real comment to a real case's history.
Baseline: PASS. **Criteria:** repeating this on every automated run would
spam the case's audit trail forever with no way to retract; only do it
deliberately, once, when you need to check the comment endpoint's
`consoleUrl`.

## `cases assign <id> --user ...` / `cases unassign <id>`

```
cx -p kb-demo -o <fmt> --yes cases assign <case-id> --user <email>
cx -p kb-demo -o <fmt> --yes cases unassign <case-id>
```

**Why manual:** reassigns a real case's owner, which can affect who's
paged/notified. Baseline: PASS for both. **Criteria:** only run against a
case where you've confirmed reassigning (even temporarily, even back to the
same user) won't confuse a real on-call workflow.

## `cases acknowledge <id>` / `cases unacknowledge <id>`

```
cx -p kb-demo -o <fmt> --yes cases acknowledge <case-id>
cx -p kb-demo -o <fmt> --yes cases unacknowledge <case-id>
```

**Why manual:** flips a real case's acknowledgment state, which is part of
real incident-response signaling. Baseline: PASS for both. **Criteria:**
same as assign/unassign -- fine as a one-off deliberate check, not a
standing automated toggle.

## `cases set-priority <id> --priority P2` / `cases clear-priority <id>`

```
cx -p kb-demo -o <fmt> --yes cases set-priority <case-id> --priority P2
cx -p kb-demo -o <fmt> --yes cases clear-priority <case-id>
```

**Why manual:** changes a real case's priority, which can affect SLA/
escalation behavior downstream. Baseline: PASS for both. **Criteria:** same
reasoning as the other toggles above.

## `cases resolve <id> --reason ...`

```
cx -p kb-demo -o <fmt> --yes cases resolve <case-id> --reason "PR176 console-link smoke test - resolving real demo case"
```

**Why manual:** resolves a real case. Baseline: PASS, with the explicit
note "Resolved a real demo-team case (CASE-43/96/97) as part of
console-link verification, per explicit approval to run everything."
**Criteria:** only run with the same kind of explicit, informed human
approval given last time -- never as an unattended replay.

## `cases close <id>`

```
cx -p kb-demo -o <fmt> --yes cases close <case-id>
```

**Why manual:** closes a real case, and CLOSED has **no undo** on this
backend. Baseline: PASS, with the explicit note "Closed a real demo-team
case (CASE-43/96/97) as part of console-link verification, per explicit
approval to run everything." **Criteria:** this is the single highest-risk
item in the whole `cases` group -- require explicit, case-specific human
sign-off every time, never bundle it into a routine re-run.

---

<a id="cleanup"></a>

# cleanup -- manual verification items

## Summary

None. `cleanup` is a single, no-argument, idempotent local housekeeping
command with no console link and no side effects beyond deleting its own
stale spill files. All 3 entries in `OLD_DIR/results/cleanup.jsonl` (text,
json, agents) are `PASS`, and `automated/cleanup.py` replays all three
exactly.

---

<a id="completions"></a>

# completions - manual verification items

## `completions generate elvish`

**Command:**

```
cx -p kb-demo -o text completions generate elvish
```

**Why it needs judgment:** FAILed last time with `Error: Shell 'elvish' is not supported
by cx completions`. This looks like a deliberate, stable input-validation rejection (not
a flaky network/backend issue - `completions` makes no API calls at all), but per policy
any FAIL still needs a human to confirm "same known limitation" vs. "something changed"
before being treated as an expected, permanent non-issue.

**Decision criteria:**
- Re-run and compare the exact error text. Unchanged -> elvish is still an intentionally
  unsupported shell, no action needed.
- If it now succeeds (prints a completion script) or the error message/wording changes
  -> check whether elvish support was added or the error handling changed, and consider
  promoting `generate elvish` into `automated/completions.py`'s format loop.

**Known baseline** (from `OLD_DIR/results/completions.jsonl`):
- status: `FAIL`
- stderr: `Error: Shell 'elvish' is not supported by cx completions`

## `completions refresh`

**Command:**

```
cx -p kb-demo -o text completions refresh
```

**Why it needs judgment:** `refresh` has no `--path`/scratch override (confirmed via
`--help` in the original run) - it regenerates ALL previously-installed completions
tracked in the real `~/.cx/config.toml` under `managed_completions`. On a real machine
this includes the user's actual registered shell completion file (e.g. `~/.zfunc/_cx`
for zsh). Running it would silently overwrite that real file. The old run explicitly
skipped this subcommand for exactly this reason, and it must stay skipped in any
mechanical replay - there is no throwaway target to redirect it to.

**Decision criteria:**
- Only test this manually, on a machine/environment where you've confirmed
  `~/.cx/config.toml`'s `managed_completions` list contains no real, depended-upon
  completion file paths (e.g. a fresh container/VM), or where you're prepared to
  regenerate/restore whatever it touches afterward.
- Never wire this into an unattended/automated script.

**Known baseline** (from `OLD_DIR/results/completions.jsonl`):
- status: `SKIPPED`
- notes: "refresh has no --path/scratch override (see --help) - it regenerates ALL
  previously-installed completions tracked in ~/.cx/config.toml managed_completions,
  which includes the user's real zsh entry at ~/.zfunc/_cx. Would overwrite the user's
  real completion file, so skipped per task instructions."

---

<a id="dashboards"></a>

# dashboards -- manual verification items

## Summary

None. Every `cx dashboards` subcommand exercised in the original PR176 run
(`OLD_DIR/results/dashboards.jsonl`) came back `PASS` with no `FAIL`/`SKIPPED`
entries -- `create`, `get`, `check`, `replace`, `catalog`, `search`,
`query-search --description`, `query-search --field`, `folders create`,
`folders list`, `folders delete`, and `delete` are all covered by
`automated/dashboards.py` with a fresh create->verify->delete cycle each run.

There is nothing in this group's baseline data that requires human/LLM
judgment to safely replay. If a future run of `automated/dashboards.py`
produces a `FAIL` where the baseline below shows `PASS`, that is a genuine
regression signal worth escalating to a human, since none was expected.

## Baseline (for regression comparison only)

All of the following were `PASS` against profile `kb-demo` on 2026-08-03:

- `dashboards create --from-file <dashboard.json>` -- `consoleUrl`/"View in
  Coralogix" present on stderr.
- `dashboards get <id>` (text/json/agents) -- consoleUrl present in all three.
- `dashboards check <id>` (text/json/agents) -- link line printed on stderr in
  all formats; `consoleUrl` field itself only appears in the text-rendered
  stderr banner, not embedded in the json/agents payload body (this asymmetry
  was noted but not flagged as a defect -- `check` is a validation-style call).
- `dashboards replace --from-file <dashboard_replace.json>` (text/json/agents)
  -- link line printed on stderr in all formats.
- `dashboards catalog` / `search` / `query-search --description` /
  `query-search --field` (all text/json/agents) -- no consoleUrl expected
  (listing/search ops), none present. Matches expectation.
- `dashboards folders create --name <name>` -- created successfully, no
  consoleUrl expected/present (folders have no console page).
- `dashboards folders list` (text/json/agents) -- no consoleUrl, as expected.
- `dashboards folders delete <id>` / `dashboards delete <id>` -- cleanup,
  both succeeded.

---

<a id="dataprime"></a>

# dataprime -- manual verification items

## Summary

None. All 9 entries in `OLD_DIR/results/dataprime.jsonl` are `PASS`,
read-only (2 local reference-data lookups + 1 live query with no side
effects). `automated/dataprime.py` replays all of them exactly.

---

<a id="docs"></a>

# docs -- manual verification items

Every subcommand in this group failed in the original run, all with the
same root cause, and none has ever produced a working invocation to
mechanically replay -- `automated/docs.py` is a no-op stub.

## 1. `docs search`

**Exact command:**
```
cx -p kb-demo -o text docs search explore spans --limit 5
cx -p kb-demo -o json docs search explore spans --limit 5
cx -p kb-demo -o agents docs search explore spans --limit 5
```
(the `<QUERY>` argument is really the single string `"explore spans"`; the
JSONL's space-joined `command` field just renders it unquoted.)

**Why it needs judgment:** all 3 formats fail identically with `Error: HTTP
403 Forbidden for https://coralogix.com/docs/llms.txt`. That's the docs
website itself (not the Coralogix product API) rejecting the CLI's outbound
HTTP request -- most likely a bot-protection/User-Agent/rate-limit block on
`coralogix.com`, not a cx-cli or backend API bug. Whether this is still
happening, has gotten worse (e.g. a different error), or has cleared up
(a real docs site policy is outside this repo's control and can change
without any code change here) can only be determined by looking at a fresh
run's actual error and comparing it to the baseline below -- that
side-by-side judgment is exactly what keeps this in `manual/`.

**Decision criteria:**
- Same `HTTP 403 Forbidden for https://coralogix.com/docs/llms.txt` on
  rerun -> still the same known, external, pre-existing issue. Not a
  regression, nothing to act on.
- A *different* error (different status code, timeout, DNS failure, or an
  actual 200 with real search results) -> investigate; either the docs site
  changed its policy (good news, promote `docs search`/`docs fetch` into
  `automated/docs.py` once confirmed stable) or something in the CLI's
  request (User-Agent, headers, retry logic) changed and needs a look.

**Baseline (2026-08-03):** `FAIL` for all 3 formats, exit 1: `Error: HTTP
403 Forbidden for https://coralogix.com/docs/llms.txt`.

## 2. `docs fetch`

**Exact command:**
```
cx -p kb-demo -o text docs fetch user-guides/data_exploration/spans/
cx -p kb-demo -o json docs fetch user-guides/data_exploration/spans/
cx -p kb-demo -o agents docs fetch user-guides/data_exploration/spans/
```

**Why it needs judgment:** same external-403 story as `docs search` above,
against a different URL (`https://coralogix.com/docs/user-guides/data_exploration/spans/index.md`).
The path itself (`user-guides/data_exploration/spans/`) was never confirmed
real either -- it was taken from the command's own `--help` example text
because `docs search` (which is supposed to supply real suffixes to feed
into `fetch`) never returned anything to copy a suffix from. So even if the
403 clears up, a fresh run needs a human/LLM to sanity-check that the
suffix path is still valid documentation, not just that the HTTP call
stopped erroring.

**Decision criteria:** same as `docs search` above -- same 403 = same known
issue; anything else = investigate. Additionally, if it starts returning
200 with real markdown content, confirm the returned page is topically
sensible for `user-guides/data_exploration/spans/` before treating this as
"fixed" -- if `docs search` also starts working, prefer feeding a
freshly-searched suffix into `fetch` rather than trusting the hardcoded
`--help`-example path indefinitely.

**Baseline (2026-08-03):** `FAIL` for all 3 formats, exit 1: `Error: HTTP
403 Forbidden for https://coralogix.com/docs/user-guides/data_exploration/spans/index.md`.
Notes: "path taken from --help example since `docs search` failed with
HTTP 403 (see search entries) so no real path was obtainable from search
output".

---

<a id="e2m"></a>

# e2m - manual verification items

## `e2m labels-cardinality`

**Command:**

```
cx -p kb-demo -o <text|json|agents> e2m labels-cardinality
```

**Why it needs judgment:** FAILed in all three formats last time with a 400 from the
backend saying a `query` field is required, yet the cx CLI's implementation
(`src/commands/e2m/api.rs::labels_cardinality`) sends a bare GET to
`/mgmt/openapi/5/events2metrics/labels/v2/cardinalities` with no query params or body at
all. This could be a genuine API-contract mismatch in the CLI (the endpoint may require a
query param the CLI never exposes as a flag) rather than a backend-only limitation -
worth a closer look at whether this command has ever worked, and whether a `--query`
flag should be added.

**Decision criteria:**
- Re-run `e2m labels-cardinality` fresh. If it's still exactly this 400 "query is
  required" error -> same known gap, no regression; consider filing a real bug/feature
  request to add a query parameter/flag to this subcommand.
- If it now succeeds or fails with a different error -> investigate as a genuine change
  (regression or backend fix); if it starts working, promote it into
  `automated/e2m.py`.

**Known baseline** (from `OLD_DIR/results/e2m.jsonl`):
- status: `FAIL` (text/json/agents)
- stderr: `Fetching E2M labels cardinality...\nError: profile 'kb-demo' failed\n\nCaused by:\n    API request failed (400): Bad Request: {"message":"Bad Request","status":400,"details":[{"code":"invalid_type","expected":"object","received":"undefined","path":["query"],"message":"Required"}]}`

---

<a id="enrichments"></a>

# enrichments - manual verification items

## `enrichments add` / `enrichments overwrite` / `enrichments remove`

**Command** (last known-tested shape):

```
cx -p kb-demo --yes enrichments add --from-file payloads/enrichments_add.json
# payload: {"requestEnrichments":[{"fieldName":"cxclipr176testfield","enrichmentType":{"suspiciousIp":{}}}]}
cx -p kb-demo -o <text|json|agents> --yes enrichments overwrite --from-file payloads/enrichments_add.json
cx -p kb-demo --yes enrichments remove --from-file payloads/enrichments_add.json
```

**Why it needs judgment:** `add` created two enrichment stubs (ids `244123`, `244124`,
fieldName `cxclipr176testfield`, type `suspiciousIp`). `remove`, given the exact same
payload shape (and 7+ other shapes tried: `id`, `enrichmentIds[]`, `ids[]`, raw array,
`enrichments[]`), always returns 200/exit-0 claiming success but a follow-up `list`
confirms the stubs are still present - `remove` appears to be a non-functional no-op
against this endpoint in this environment. There is no confirmed working delete route,
so automating `add` would create a new permanent orphan on every run. `overwrite`
separately FAILs with a 400 "enrichmentType is required" even though `enrichmentType` is
present and well-formed - looks like a genuine backend validation bug.

**Decision criteria:**
- Before touching this, run `enrichments list` and check whether ids `244123`/`244124`
  (fieldName `cxclipr176testfield`) are still present from the original run - if so, this
  environment already carries that residue; do not add more without a real plan to clean
  them up (e.g. escalate to the Coralogix UI or API owner for a manual delete route).
- If retrying `remove` against a *fresh* stub you just added: try the exact shapes above.
  If any of them actually removes the entry (confirmed via `list`), that's a real fix on
  the backend - update `automated/enrichments.py` to include the working `add`/`remove`
  pair and delete this section.
- If retrying `overwrite`: if the 400 "enrichmentType is required" error is gone or
  changed, that's worth investigating as a real regression/fix, not assumed benign.

**Known baseline** (from `OLD_DIR/results/enrichments.jsonl`):
- `overwrite` status: `FAIL` (text/json/agents) - notes: "API rejects with 400
  'enrichmentType is required' even though enrichmentType is present and well-formed in
  the request body (tried suspiciousIp{} and geoIp{withAsn} variants). Likely a backend
  validation bug in AtomicOverwriteEnrichments unrelated to the cx CLI/PR176."
- `remove (cleanup attempt)` status: `PASS` (i.e. exit 0) but functionally a no-op -
  confirmed via `list (post-cleanup verification)` still showing ids 244123/244124 with
  fieldName `cxclipr176testfield` and `enrichmentType.suspiciousIp: {}`.

## `enrichments custom search`

**Command:**

```
cx -p kb-demo -o <text|json|agents> enrichments custom search --id <id> --query testkey
```

**Why it needs judgment:** FAILed with a 404 last time even against a freshly-created,
confirmed-existing table (verified via `get`/`list`, and after a delay) - looks like an
environment/API-level limitation, not a cx bug, but that needs re-confirming against a
fresh table rather than assumed.

**Decision criteria:** create a throwaway custom enrichment table (same shape as
`automated/enrichments.py`'s custom lifecycle), populate it, then run `custom search
--id <id> --query testkey`:
- Still 404 Not Found -> same known environment limitation, no action. Clean up the table.
- 200 with results (or a different error) -> the search endpoint now works or fails
  differently; update this note and consider promoting `custom search` into
  `automated/enrichments.py`.

**Known baseline** (from `OLD_DIR/results/enrichments.jsonl`):
- status: `FAIL` (text/json/agents)
- notes: "Search returns 404 Not Found even against a freshly-created, confirmed-existing
  table (verified via get/list) and after a delay - appears to be an environment/API-level
  limitation unrelated to cx CLI/PR176."

---

<a id="iam"></a>

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

---

<a id="integrations"></a>

# integrations -- manual items

## `integrations definition slack-central`

```
cx -p kb-demo -o <text|json|agents> integrations definition slack-central
```

**Why manual:** FAILed on all 3 formats with no note recorded explaining
why -- unlike the top-level `create`/`test`/`update`/`delete` 501s (which
are clearly pinned to a documented backend limitation), this 404 was never
diagnosed. It's odd on its face: `integrations get slack-central` (the
"get instance" call) succeeds against the same id, but `definition` (which
presumably should return the integration's schema/definition) 404s.
Deciding whether that's expected (e.g. `definition` requires a catalog key,
not an instance id) or a real gap needs a human to actually read the
command's help/source, which is exactly the kind of judgment call
automation can't make.

**Known baseline** (`OLD_DIR/results/integrations.jsonl`, status FAIL, exit 1):

```
stderr: Fetching integration definition slack-central...
Error: profile 'kb-demo' failed

Caused by:
    API request failed (404): Not Found: Not Found
```

**Decision criteria:** if `integrations definition <catalog-key>` (e.g.
`slack-central`'s catalog type, not its instance id) works, that confirms
`definition` wants a catalog key like `extensions get`/`contextual-data
get` do -- document that and consider promoting to automated with the right
key. If it still 404s against every kind of key, that's worth a real bug
report.

## `integrations template`

```
cx -p kb-demo -o <text|json|agents> integrations template
```

**Why manual:** FAILed with "Integration type by integration key not
found: template" -- reads like `template` requires a positional/flag
argument (an integration type key) that the original session never
supplied, rather than a backend outage. Confirming that (vs. a real gap)
means reading `--help` or the command source, which is a judgment call.

**Known baseline:**

```
stderr: Fetching integration template...
Error: profile 'kb-demo' failed

Caused by:
    API request failed (404): Not Found: Integration type by integration key not found: template
```

**Decision criteria:** check `cx integrations template --help`; if it takes
a required key argument, re-test with a real one (e.g. `slack-central`) and
promote to automated if that works cleanly.

## `integrations extensions deploy / update / undeploy`

Three payload shapes were tried for `deploy` and none worked; `update` and
`undeploy` were probed with the same payloads and also failed. No
known-good shape was ever found, so there's nothing safe to mechanically
replay.

```
cx -p kb-demo -o json --yes integrations extensions deploy --from-file extension_deploy_v1.json   # {"id": "AIkido", "version": "0.0.1"}
cx -p kb-demo -o json --yes integrations extensions deploy --from-file extension_deploy_v2.json
cx -p kb-demo -o json --yes integrations extensions deploy --from-file extension_deploy_v3.json
cx -p kb-demo -o json --yes integrations extensions update --from-file extension_deploy_v3.json
cx -p kb-demo -o json --yes integrations extensions undeploy --from-file extension_deploy_v1.json
```

**Known baseline** (all FAIL, exit 1, identical error shape regardless of
payload):

```
stderr: ...
Error: profile 'kb-demo' failed

Caused by:
    API request failed (404): Not Found: Not Found
```

**Decision criteria:** figure out the correct schema (check `src/commands/
integrations/api.rs`, or try a catalog id that's known to already be
deployable, e.g. whatever powers "Slack" in `extensions get`/`deployed`).
If a working deploy payload is found, this whole subgroup
(deploy->update->undeploy, self-cleaning) can be promoted to automated. If
it still 404s the same way, that's the same known gap, not a regression.

## `integrations contextual-data create`

Two payload shapes were tried (`id`+`parameters` and `key`+`parameters`)
and both were rejected at the proto layer for an unknown field, with no
schema hint in the CLI output.

```
cx -p kb-demo -o json --yes integrations contextual-data create --from-file contextual_data_create_v1.json
# {"id": "StatusPage-Tracker", "version": "0.0.1", "parameters": [...]}
#   -> 400 Bad Request: proto: (line 1:2): unknown field "id"

cx -p kb-demo -o json --yes integrations contextual-data create --from-file contextual_data_create_v2.json
# {"key": "StatusPage-Tracker", "version": "0.0.1", "parameters": [...]}
#   -> 400 Bad Request: proto: (line 1:2): unknown field "key"
```

**Why manual:** no known-good payload shape exists to mechanically replay.
Note this doesn't block `contextual-data get`/`definition` against
"StatusPage-Tracker" -- those are catalog-key lookups that work regardless
of whether a real deployed instance exists (confirmed automated in
`../automated/integrations.py`).

**Decision criteria:** if the correct field name is found (try `key` vs
`id` variants further, or check `src/commands/integrations/api.rs`) and
`create` returns 200, wire up a full create->get->test->delete lifecycle in
automated (check first whether a working delete route exists for
contextual-data instances, mirroring the ai-center custom-evaluations
caution above).

## `integrations contextual-data test StatusPage-Tracker`

```
cx -p kb-demo -o <text|json|agents> --yes integrations contextual-data test StatusPage-Tracker
```

**Why manual:** directly downstream of the unresolved `create` above --
tested against the catalog key since no deployed instance exists, and
failed because of that.

**Known baseline** (FAIL, exit 1, all 3 formats):

```
stderr: Testing contextual data integration StatusPage-Tracker...
Error: profile 'kb-demo' failed

Caused by:
    API request failed (400): Bad Request: Request errors:
    * Unknown integration metadata specific data: Empty
```

**Decision criteria:** re-test once `contextual-data create` has a known
working payload and a real deployed instance exists; until then this
result is expected and not a new regression.

---

<a id="logs"></a>

# logs -- manual verification items

## Summary

None. All 5 entries in `OLD_DIR/results/logs.jsonl` are `PASS`, read-only
DataPrime queries with no side effects. `automated/logs.py` replays all of
them exactly (the 3-format matrix, the explicit `--tier frequent` check, and
the explicit default-tier check).

---

<a id="metrics"></a>

# metrics -- manual verification items

## Summary

None. All 12 entries in `OLD_DIR/results/metrics.jsonl` are `PASS`,
read-only PromQL queries with no side effects. `automated/metrics.py`
replays all of them exactly (`query`, `query-range`, `search --name`,
`get-labels`, each across all 3 output formats).

Note: `container_memory_usage_By` is assumed to remain a valid metric name
on the `kb-demo` team. If it's ever removed/renamed, `search --name
container_memory*` (also replayed here) is the fastest way to find its
replacement -- that substitution is mechanical, not a judgment call, so it
doesn't promote this group to manual.

---

<a id="notifications"></a>

# notifications -- manual verification items

## 1. `notifications test preset`

**Command shape:** unknown -- no working request body was ever found.

**Why it needs judgment:** the original session tried `connectorId` +
`entityType` (accepted) plus various guesses for identifying *which* preset
to render (`presetId`, `preset`, `configOverrides[].id`), all rejected by the
proto validator as unknown fields; omitting an identifier entirely produced a
generic "not a valid identifier" 400. Finding the right field name requires
either reading the service source/proto (explicitly out of scope for the
original probing session) or further live trial-and-error against the API,
which is exactly the kind of guessing an unattended script should not do
silently.

**Decision criteria:** if picking this up again, either (a) find the
request proto/OpenAPI spec for this endpoint and read the correct field name
directly, or (b) resume probing live with `--yes -o json` and small payload
tweaks, one field at a time, recording every attempt (success or 400) via
`record()` the way `step7_slos.py` did for the SLO schema discovery. Once a
working shape is found, promote it into `automated/notifications.py` and this
section can be deleted.

**Baseline (2026-08-03):** `SKIPPED`. Notes: "Could not determine the
required JSON request schema after multiple probing attempts (connectorId +
entityType accepted, but presetId/preset/configOverrides[].id all rejected as
unknown fields by the underlying proto validator, while omitting an
identifier yields a generic 'not a valid identifier' 400); no source/docs
reference was consulted per task constraints (no reading src/). Skipped
rather than continuing to guess."

## 2. `notifications test routing-condition`

**Command shape:** unknown -- same situation as `test preset`.

**Why it needs judgment:** `entityType`/`entityLabels` were accepted, but
every guess for the condition expression itself (`condition`, `expression`,
`matcher`, `routingCondition`, `entityMatcher`, `conditionExpression`) was
rejected as an unknown field. Same reasoning as above: needs either a schema
reference or further live probing.

**Decision criteria:** same as `test preset` above -- find the field name
(spec or live probing), verify it renders a routing condition, then promote
into the automated script.

**Baseline (2026-08-03):** `SKIPPED`. Notes: "Could not determine required
JSON field name for the condition expression: entityType/entityLabels were
accepted, but condition/expression/matcher/routingCondition/entityMatcher/
conditionExpression were all rejected as unknown fields by the proto
validator. No source/docs reference consulted per task constraints. Skipped
rather than continuing to guess."

## Not a manual item, but worth knowing

`connectors entity-types` and `connectors entity-subtypes --type SLACK` are
in `automated/notifications.py` even though they `FAIL` every time (404 "not
found", as if the CLI is trying to `GET` a connector whose id is literally
the string `"entity-types"`). That failure is fully pinned to a known cause
and read-only, so it's safe to mechanically replay -- no judgment needed on
each run. Only escalate if a rerun ever shows a *different* error or an
unexpected `200`, since that would mean the underlying routing bug changed
shape or got fixed.

**Baseline (2026-08-03) for that comparison:**
- `connectors entity-types` (all 3 formats) -- FAIL, exit 1: `API request
  failed (404): Not Found: {"reason":"Connector with id entity-types not
  found"}`.
- `connectors entity-subtypes --type SLACK` (all 3 formats) -- FAIL, exit 1:
  `API request failed (404): Not Found: Not Found`.

---

<a id="olly"></a>

# olly - manual verification items

## `olly ask`

**Command:**

```
cx -p kb-demo -o json olly ask "What services have I got?"
```

**Why it needs judgment:** this is a real call to the Coralogix AI assistant - it costs
real tokens every time it runs, and the response is inherently non-deterministic (it's a
live LLM answer over this team's real telemetry). The old run intentionally limited
itself to a single exchange in `json` format only, and explicitly skipped repeating the
call in `text`/`agents` format "to avoid repeated real AI cost" - the same reasoning
means it should not be re-run automatically/repeatedly just to regenerate console-link
verification data.

There's an added wrinkle: the artifact-looking id embedded in the `ask` response's
markdown link (`8ae1d5fb-e388-4a5c-a4e3-9836f8a0c8f9`) 404s against `olly artifacts get`
- it appears to reference a different resource type (a UI-side insight/service-map view),
not a stored artifact. A human needs to look at whatever a fresh `ask` response contains
and judge whether its embedded links/ids make sense, rather than mechanically chasing
whatever id shows up.

**Decision criteria:**
- If re-verifying this command, do it as a single deliberate call (not looped/repeated),
  and check: does the response include a `chat_id`? Does it include a `consoleUrl`? Does
  any artifact-style link in the response actually resolve via `olly artifacts get`?
- If a linked id now resolves via `artifacts get` (i.e. the mismatch noted above is
  fixed), that's worth noting as a behavior change but doesn't need urgent action.
- Do not build a loop or scheduled job that calls `olly ask` repeatedly for verification
  purposes - the cost and non-determinism make it unsuitable for that.

**Known baseline** (from `OLD_DIR/results/olly.jsonl`):
- `ask` (json): `PASS` - notes: "Real AI assistant call (costs tokens) -- limited to one
  exchange in json format only per instructions to avoid repeated real AI cost."
- `ask (text format)` / `ask (agents format)`: `SKIPPED` - "Skipped repeat real-AI-cost
  call; already verified consoleUrl + response shape via the json-format run above."
- `artifacts get` (using the id from the `ask` response, `8ae1d5fb-...`): `FAIL` (all
  formats) - see `artifacts get (real pre-existing artifact id)` entry which succeeded
  instead using a real id (`30fbcbe6-...`) taken from `artifacts list` - notes: "The
  artifact id linked from the `olly ask` response 404'd on this endpoint (likely a
  different resource type, e.g. UI insight link, not a stored artifact); used a real id
  from `artifacts list` instead."

---

<a id="parsing-rules"></a>

# parsing-rules - manual verification items

## `parsing-rules bulk-delete`

**Command** (last known-tested shape):

```
cx -p kb-demo -o <text|json|agents> --yes parsing-rules bulk-delete --ids <id1> <id2>
```

**Why it needs judgment:** it FAILed in all three output formats last time with a real
backend error, not a cx-side bug. Re-running a call we already know will fail is not a
useful mechanical pass/fail check - a human needs to check the *current* backend response
to decide "still the same known 501" vs "something changed" (e.g. it now works, or fails
differently).

**Decision criteria:** create two throwaway rule groups (same shape as
`automated/parsing-rules.py`'s single-create flow, just twice), run `bulk-delete --ids
<a> <b>`, and compare:
- Still exit 1 with `API request failed (501): Unimplemented: Method Not Allowed` -> same
  known backend limitation, no action needed. Clean up both groups individually via
  `parsing-rules delete`.
- Anything else (200, a different error code/message, or a cx-side panic/parse error) ->
  real regression or a backend feature that shipped since - investigate and update this
  note.

**Known baseline** (from `OLD_DIR/results/parsing-rules.jsonl`):
- status: `FAIL` (all of text/json/agents)
- stderr: `[auto-approved via --yes] Bulk delete parsing rules?\nBulk deleting rule groups...\nError: profile 'kb-demo' failed\n\nCaused by:\n    API request failed (501): Unimplemented: Method Not Allowed`
- notes: (none recorded beyond the stderr itself)

---

<a id="profiles"></a>

# profiles -- manual verification items

## 1. `profiles add` (and the `set-default`/`delete` steps that depend on it)

**Command:**
```
cx profiles add cx-harness-test
```
(no `-p kb-demo` -- `profiles add` is a local, profile-agnostic command)

**Why it needs judgment:** `profiles add` is fully interactive -- it drives
an `inquire`-based TUI wizard (`Select`/`Password`/`Text`/`Confirm` prompts)
with **no non-interactive flags** (confirmed via `--help`: only `[NAME]` and
`--set-default` exist). Piped/non-TTY stdin fails immediately with `input
device is not a TTY`. The original run drove it with `expect` sending raw
keystrokes (Down+Enter, typed text, more Down+Enter, plain Enter to accept
defaults, `n`+Enter) -- but the JSONL's `command` field only records a prose
description of that interaction (`"expect-driven: ... (Authentication
method=API key, key=dummy-test-key-12345, Region=eu2, Label=<empty>,
storage=file, output format=text, set-as-default=No)"`), not the literal
expect script or keystroke sequence. There is nothing mechanical to copy —
reconstructing and trusting a TUI-driving script (`expect`, `pty`, or
`pexpect`) without ever being able to run it first is exactly the kind of
judgment call this file exists for.

**Reconstructed prompt flow** (read directly from
`src/commands/profiles/mod.rs::run_add` / `configure_api_key`, current as of
this session -- use this as the starting point, not as a pre-verified
script):

1. `Authentication method:` -- `Select`, starts on `OAuth (browser login)`
   (cursor 0). Send **Down, Enter** to land on `API key (paste manually)`.
2. `Coralogix API key (Team Key or Personal Key):` -- `Password` (masked,
   no confirmation). Type a dummy value (e.g. `dummy-test-key-<suffix>`),
   **Enter**.
3. `Region:` -- `Select` over `["us1","us2","us3","eu1","eu2","ap1","ap2","ap3"]`,
   `with_starting_cursor(2)` (index 2 = `us3`; the inline `// eu1` comment
   in the source is stale/wrong for the current array order -- don't trust
   it, trust the actual index). To land on `eu2` (index 4): **Down, Down,
   Enter**.
4. `Label (e.g. 'prod'):` -- `Text::prompt_skippable`. Just **Enter** with
   no text -- submits `""`, which the code filters to `None`.
5. `Where should API keys be stored?` -- `Select`, starts on `file` (cursor
   0, the desired choice). **Enter**.
6. `Default output format:` -- `Select` over `["text","json","agents"]`,
   starting cursor = index of the *global* `default_output_format`
   (`~/.cx/config.toml`, currently `"text"` on this machine -> index 0, the
   desired choice). **Enter**.
7. `Set 'cx-harness-test' as the default profile?` -- `Confirm`, default
   `false`. Send **`n`, Enter** (or just Enter, since default is already
   No) to decline.

Expect exit 0 and stdout ending `Profile 'cx-harness-test' saved to
~/.cx\nCredentials stored in profile file`.

**Decision criteria:** before trusting any reconstructed automation of this
flow unattended, a human/LLM needs to actually run it once, watch the TUI,
and confirm each prompt lands where expected (cursor positions and prompt
text can silently drift as `inquire`/the option lists change -- e.g. the
stale `// eu1` comment above is proof this has already happened once).
Once a keystroke sequence is confirmed working end-to-end (add -> profile
file written correctly -> `set-default` -> `delete -f`), it can be promoted
into `automated/profiles.py` using `expect` (available at `/usr/bin/expect`
on this machine) or Python's `pty` module.

`set-default cx-harness-test` / `set-default c4c` (to restore) / `delete
cx-harness-test -f` are each trivial, non-interactive, one-liner commands
on their own (`profiles set-default <name>`, `profiles delete <name> -f`) --
they only stay manual because they have nothing to target without a
successful `add` first. `list` (the read-only, argument-free part of this
group) is already covered in `automated/profiles.py`.

**Baseline (2026-08-03):**
- `add` -- `PASS`, exit 0. Notes: "profiles add is fully interactive
  (dialoguer prompts), requires a real TTY ... Drove it with `expect`
  sending: Down+Enter (select API key auth), type
  dummy-test-key-12345+Enter, type eu2+Enter (region), Enter (skip label),
  Enter (keep file storage), Enter (keep text output format), 'n'+Enter (do
  not set as default). Verified profile written correctly to
  ~/.cx/profiles/cx-harness-test.toml."
- `set-default (to throwaway)` -- `PASS`, exit 0.
- `set-default (restore c4c)` -- `PASS`, exit 0. Notes: "restoring real
  default profile back to c4c after throwaway set-default test".
- `list (verify c4c restored as default before cleanup)` -- `PASS`, exit 0.
- `delete (cleanup)` -- `PASS`, exit 0 (`profiles delete cx-harness-test -f`).

This machine's `~/.cx/config.toml` currently shows `default_profile = "c4c"`,
confirming the original run's restore step left things clean.

---

<a id="recording-rules"></a>

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

---

<a id="retentions"></a>

# retentions - manual verification items

## `retentions activate` (IRREVERSIBLE - already flipped on kb-demo)

**Command:**

```
cx -p kb-demo -o json --yes retentions activate
```

**Why it needs judgment:** this call flips `enableTags` from `false` to `true` for the
whole team. The old run already executed it once (deliberately, "run once per
instructions") and then tried to revert it via `retentions update` - which 501s on this
team/plan, and there is no `deactivate`/opposite subcommand exposed by the CLI. So
`enableTags` is now permanently `true` on the "kb-demo" team with no way back through the
CLI. **This must never be auto-replayed** - on kb-demo it would be a harmless re-flip of
an already-true flag, but the whole point of keeping this manual is to stop the pattern
of running it unattended: on any other team (or if kb-demo's config is ever fixed
upstream) it would flip another flag with no revert path.

**Decision criteria:**
- Do not run `retentions activate` against any team unless a human has explicitly
  decided the resulting irreversible state change is acceptable for that team.
- If investigating this area, use `retentions status` (read-only, safe, in
  `automated/retentions.py`) to check current state first.

**Known baseline** (from `OLD_DIR/results/retentions.jsonl`):
- `status (baseline, before activate)`: `PASS` - notes: "BASELINE ORIGINAL VALUE:
  enableTags=false. Recorded for manual revert reference."
- `activate`: `PASS` (exit 0) - notes: "LIVE MUTATION, run once per instructions. ...
  Changed enableTags false -> true. Attempted to revert via `retentions update` (see next
  entry) but that endpoint returns 501 Unimplemented on this team, and there is no
  `deactivate`/opposite subcommand exposed by the CLI, so enableTags COULD NOT BE
  RESTORED to its original false value. This is a real residual side effect on the live
  demo team: enableTags is now true. Flagging for the user."
- Current state on kb-demo going forward: `enableTags=true` (confirmed by the `status`
  calls immediately after `activate`, and expected to still read `true` on any fresh
  `automated/retentions.py` run).

## `retentions update` (known 501 baseline)

**Command:**

```
echo '{"retentions":[{"id":"5ed15614-4f2a-47f6-b8e2-5e6e29a3410c","order":2,"name":"Short","editable":true}]}' \
  | cx -p kb-demo -o json --yes retentions update
```

**Why it needs judgment:** stable, known 501 Unimplemented for this team/plan, confirmed
with both a real-looking body and an empty `{}` body. This is also the same endpoint that
would have been the only way to revert `activate` above, so it's doubly relevant to
re-check if this team's plan is ever upgraded.

**Decision criteria:** re-test manually if you need to confirm whether this team's plan
now supports the endpoint (e.g. before attempting to use it to finally revert the
`enableTags` flip); otherwise leave as-is, it's a stable known limitation.

**Known baseline** (from `OLD_DIR/results/retentions.jsonl`):
- status: `FAIL`
- notes: "Backend returns 501 Unimplemented for this team/plan regardless of payload
  shape (also tried with an empty {} body - same 501). This looks like a genuine backend
  limitation (feature not enabled for this demo team), not a bug introduced by PR176's
  console-link changes. Could not use this to revert the `activate` mutation above."

---

<a id="schema"></a>

# schema -- manual verification items

## Summary

None. `schema` is a single, no-argument, read-only local command (dumps the
CLI's command tree as JSON for agent discovery) with no console link and no
side effects. All 3 entries in `OLD_DIR/results/schema.jsonl` (text, json,
agents) are `PASS`, and `automated/schema.py` replays all three exactly.

---

<a id="search-fields"></a>

# search-fields -- manual verification items

## Summary

None. All 6 entries in `OLD_DIR/results/search-fields.jsonl` are `PASS`,
read-only Olly Knowledge Base semantic/value field searches with no side
effects. `automated/search-fields.py` replays both queries (`semantic` and
`value` search types) across all 3 output formats exactly.

Note: the original `command` field in the JSONL joins argv with spaces for
display, which makes `search-fields http response status code --dataset
logs --limit 5` look like 4 separate positional words. It is actually a
single quoted `<TEXT>` argument (`"http response status code"`) --
confirmed against `src/main.rs`'s own `--help` examples (`cx search-fields
"http response status code"`). `automated/search-fields.py` passes it as one
argv element accordingly.

---

<a id="slos"></a>

# slos -- manual verification items

## Summary

None currently. Every subcommand actually exercised in the original PR176 run
(`list`, `create`, `get`, `update`, `delete`) ended in a `PASS` using a known
payload shape, and is fully covered by `automated/slos.py`.

The only `FAIL`/non-PASS entries in `OLD_DIR/results/slos.jsonl` were
schema-discovery noise superseded by the final working payload, or a CLI
usage mistake -- neither needs re-judgment on replay:

- `create (setup)` (first real attempt, used the pre-fix `slo_body.json`) --
  FAIL, `exit_code=1`, superseded by "create (attempt 1/2/3)" below and the
  final "create (setup, success)".
- `create (attempt 1: bad field 'serviceName')` -- FAIL: `API request failed
  (400): Bad Request: proto: (line 1:262): unknown field "serviceName"`.
  Top-level `serviceName`/`filters` are not valid fields.
- `create (attempt 2: missing 'sli')` -- FAIL: `ValidationError: "slo.sli" is
  required`. An SLI definition (`requestBasedMetricSli` or
  `windowBasedMetricSli` oneof) is mandatory.
- `create (attempt 3: metric range not [1m])` -- FAIL: `Good events query:
  SLO queries must use a 1-minute range ([1m])...`. PromQL queries inside
  `requestBasedMetricSli.goodEvents`/`totalEvents` must use `[1m]`.
- `update (attempt 1: id as positional arg)` -- FAIL, `exit_code=2`,
  `error: unexpected argument '<id>' found`. `slos update` takes no
  positional id; the id must be embedded in the JSON body.

## If this ever needs re-litigating

If `automated/slos.py`'s `create` step starts failing again with an
`unknown field` / `"slo.sli" is required` / `[1m]` range error, that means
the SLO API schema has changed since 2026-08-03 and the working payload in
`payloads/slos_slo_create.json` needs to be rediscovered the same way the
original session did (trial-and-error against the live `CreateSlo` endpoint,
reading the 400 error's `reason` field each time) -- that rediscovery step
is the one piece of this group that would need LLM/human judgment.

---

<a id="spans"></a>

# spans -- manual verification items

## Summary

None. All 3 entries in `OLD_DIR/results/spans.jsonl` are `PASS`, read-only
DataPrime queries with no side effects. `automated/spans.py` replays all of
them exactly.

---

<a id="suppression-rules"></a>

# alerts suppression-rules -- manual items

## `alerts suppression-rules update`

**Command (best-effort, from the original session):**

```
cx -p kb-demo -o <text|json|agents> --yes alerts suppression-rules update --from-file supp_update6.json
```

with `supp_update6.json`:

```json
{
  "alertSchedulerRule": {
    "id": "ddcd4a90-84af-4138-b65c-5c570d7143e7",
    "name": "pr176-smoke-test-updated",
    "filter": {
      "whatExpression": "source logs | filter true",
      "alertUniqueIds": { "value": [] }
    },
    "schedule": {
      "scheduleOperation": "SCHEDULE_OPERATION_MUTE",
      "oneTime": {
        "timeframe": {
          "startTime": "2026-08-03T19:30:00",
          "endTime": "2026-08-03T20:30:00",
          "timezone": "UTC"
        }
      }
    }
  }
}
```

**Why it needs judgment:** unlike `create` (which has a known-good payload
shape), `update` never succeeded. 11 payload variations were tried in the
original session (id at top level vs nested, `alertSchedulerRuleId` vs
`ruleId`, scheduler-wrapper vs direct `oneTime`, with/without `timezone`,
empty vs populated `alertUniqueIds`) and every one failed with the same
generic, path-less error. There is no "known-working command+payload" to
mechanically replay, which is a hard requirement for the AUTOMATED bucket.

**Decision criteria for a future pass:**
- If someone discovers (via zod-level validation messages, API docs, or
  reading `src/commands/alerts/api.rs`) the exact required shape and gets a
  real 200 back with a `consoleUrl` in the response, promote this to
  `../automated/suppression-rules.py` using that exact payload.
- If it still fails with the same `Invalid UUID format` signature below,
  that's the same known issue -- no new investigation needed, just note it
  recurred.
- If it fails with a *different* error/message, or partially succeeds, that
  is a change worth investigating (regression or partial fix) before
  deciding anything.

**Known baseline** (`OLD_DIR/results/suppression-rules.jsonl`, status FAIL,
exit 1, all 3 output formats):

> Best-effort black-box payload (id + name + filter.whatExpression/alertUniqueIds
> + schedule.scheduleOperation/oneTime.timeframe, same shape that succeeded for
> create) consistently fails with a generic, path-less "API request failed
> (400): Bad Request: Invalid UUID format" once schedule is included, even with
> a syntactically-valid UUID id and a real alert id in alertUniqueIds.value.
> zod-level validation (which gives clear field-path errors like
> [alertSchedulerRule.schedule.scheduler.oneTime.timeframe.timezone]) passes
> fully; the error surfaces only at the raw backend/proto layer with no field
> path, so the exact required shape could not be reverse-engineered via
> black-box testing in reasonable effort... Not a PR176 console-link issue --
> never got a successful (200) update response to check for a consoleUrl at
> all. Recorded as FAIL: could not exercise this subcommand successfully.

---

## Note on `list` / `get` (classified AUTOMATED, not manual, but flagged for awareness)

These two also FAILed in the original run (null-deserialized fields despite
exit 0), but the root cause is already fully diagnosed and pinned as a
pre-existing, unrelated bug -- so `../automated/suppression-rules.py` replays
them with a small deterministic parse-and-compare check instead of punting
to manual judgment. If that script ever reports the bug as "FIXED", it's
worth a human sanity check, but the day-to-day replay itself needs none.

---

<a id="tco"></a>

# tco - manual verification items

## `tco reorder`

**Command** (last known-working shape):

```
echo '{"sourceType":1,"orders":[{"id":"<throwaway-policy-id>","order":1}]}' \
  | cx -p kb-demo -o json --yes tco reorder --from-file -
```

**Why it needs judgment:** `reorder` PASSed last time with this shape
(`sourceType: <1|2|3 int enum>`, `orders: [{id, order}]`), but it doesn't just reorder the
throwaway policy - it renumbers the priority order of ALL TCO policies of that
`sourceType` for the *real* live team, since `order` values are relative positions across
the whole set. Even briefly (until the throwaway policy is deleted again), this changes
how real TCO policies are prioritized for actual log/span routing decisions. That's a
genuine, if temporary, production side effect that needs a human call each time about
whether it's safe to run against whichever real team is targeted - it is not a pure,
inert round-trip like `update`/`test`.

**Decision criteria:**
- If you have explicit sign-off to reorder policies on the target team (e.g. a true
  disposable/sandbox team where no one depends on TCO policy order), it is safe to run
  the create -> reorder -> delete sequence from `run_tco_3_mutations.py`'s pattern.
- On any team where real TCO policy order matters (including "kb-demo" if it's shared),
  treat this as unsafe to auto-run; verify manually and note the outcome here.
- Also worth double-checking: the failed discovery attempt below hints "sibling" policies
  may get auto-created by the backend on `create` under some payload shapes - if you see
  unexpected extra ids appear during a fresh `tco list` after creating one throwaway
  policy, investigate before reordering or deleting anything.

**Known baseline** (from `OLD_DIR/results/tco.jsonl`):
- `reorder (failed attempt, wrong policy id for sourceType)`: `FAIL` - notes: "Learning
  call: the auto-created sibling policy id is not a LOGS-sourceType policy, so
  sourceType=1 reorder 404s on it. Not a PR bug - just wrong test payload during shape
  discovery."
- `reorder`: `PASS` - notes: "consoleUrl present, points to #/tco-policies as expected.
  Payload shape: {sourceType:<1|2|3 int enum>, orders:[{id,order}]}."

## `tco settings-update`

**Command:**

```
echo '{"logsPolicySettings":{"defaultPriority":"PRIORITY_TYPE_HIGH"},"spansPolicySettings":{"defaultPriority":"PRIORITY_TYPE_HIGH"}}' \
  | cx -p kb-demo -o json --yes tco settings-update --from-file -
```

**Why it needs judgment:** stable, known 501 Unimplemented for this team/plan (same
backend limitation as `retentions update`), confirmed even when replaying the exact
current settings verbatim (a true no-op value). Re-running a mutating call we already
know will 501 is not a useful automated pass/fail check - only worth revisiting if the
underlying code path changes (e.g. a cx release adds new fields/validation to this
command, or the team's plan is upgraded to support it).

**Decision criteria:**
- If `cx tco settings-update` code path (`src/commands/tco/`) changed since this baseline
  was recorded, re-test manually and update this note either way (still 501, or now
  works).
- If unchanged, no action needed - this is the expected, stable state for this team/plan.

**Known baseline** (from `OLD_DIR/results/tco.jsonl`):
- status: `FAIL`
- stderr: `Error: profile 'kb-demo' failed\n\nCaused by:\n    API request failed (501): Unimplemented: Method Not Allowed`
- notes: "Backend returns 501 Unimplemented for this team/plan (same as retentions
  update) regardless of payload - tried replaying the exact current settings verbatim
  (no-op value) and still 501. Backend limitation, not a PR176 bug. No mutation occurred
  (settings unchanged, confirmed via a subsequent `tco settings` read)."

## Unexplained extra policy ids from the original run

The original run's cleanup deleted 4 policy ids, 2 of which 404'd (already gone):
`5d17efb2-648f-512c-8fd9-d1924336e5c2` and `a79e70e8-2e6f-5677-9c4a-baf4fa6156f4`. These
came from unrecorded manual discovery probing (hypothesized as "auto-created sibling
policies" of a different `sourceType`, but never confirmed) rather than the single,
fully-recorded create/delete pair (`4d67628b-...`) that `automated/tco.py` replays.
Since both extra ids 404 on delete, there is nothing live to clean up from them today.
If a fresh run ever produces an unexpected extra policy id alongside the one you asked
to create, treat it the same way: investigate what it is via `tco get <id>` before
assuming it's safe to leave or delete.

---

<a id="usage"></a>

# usage - manual verification items

None. Every `usage` subcommand exercised in the original run (`summary`, `daily`,
`logs-count`, `spans-count`, `capabilities`, `query`, `export-status`) is read-only and
PASSed in all three output formats (see `OLD_DIR/results/usage.jsonl`). All of it is
replayed mechanically in `automated/usage.py`.

---

<a id="views"></a>

# views -- manual verification items

## Summary

Nothing in this group requires per-run human/LLM judgment to safely replay
-- `automated/views.py` covers `create`, `get`, `update`, `delete`,
`folders create`, `folders list`, `folders get`, `folders update`, and
`folders delete` across all three output formats. The two things worth
flagging are both *known, pinned* issues, not open questions -- listed below
so a future reader can tell "still the same known issue" from "something
changed."

## 1. `views create` never emits a consoleUrl (in any format), even though the
   view really is created

**Exact commands (original run, ids no longer exist):**
```
cx -p kb-demo -o text  --yes views create --from-file payloads/view_create_text_r2.json
cx -p kb-demo -o json  --yes views create --from-file payloads/view_create_json_r2.json
cx -p kb-demo -o agents --yes views create --from-file payloads/view_create_agents_r2.json
```

**Why it's pinned, not open:** the original session confirmed via a
follow-up `views list` that the view genuinely gets created server-side.
The bug is specifically that (a) no `consoleUrl`/"View in Coralogix" line is
ever printed, in any of the three formats, contradicting
`docs/configuration.md` which documents views create/update as linking to
`{base}/#/explore?viewId={id}`, and (b) the command's own stdout is useless
for discovering the new id: text mode prints nothing, json mode prints `[]`,
agents mode prints an empty list -- `automated/views.py` works around this
by calling `views list` and matching on the name it just created, exactly
like the original session did.

**Decision criteria:** `automated/views.py`'s `create` step is expected to
keep showing `FAIL` (missing consoleUrl) on every rerun. If a rerun ever
shows a `consoleUrl` present for `create`, or the id becomes recoverable
straight from `create`'s own output, that's a genuine fix worth confirming
by hand (check it actually matches `{base}/#/explore?viewId={id}` in all
three formats) rather than assuming it's just noise.

**Baseline (2026-08-03):** `FAIL` (exit 0) in all three formats. Notes:
"View WAS actually created server-side (confirmed via subsequent views
list) but create command never prints 'View in Coralogix' nor emits
consoleUrl in ANY format (text/json/agents), contradicting
docs/configuration.md which lists views create/update as
{base}/#/explore?viewId={id}. Also: text-format create prints no output at
all ...; json-format prints an empty [] array ...; agents prints an empty
[0] list."

## 2. `views folders update` is a known 501 Unimplemented

**Exact command shape (original run, id no longer exists):**
```
cx -p kb-demo -o text --yes views folders update e08f84e8-8e5d-4eaa-8a9f-16dbcf82983c --from-file payloads/folder_update_text.json
```

**Why this is not really "manual":** it's a deterministic backend
limitation (method not implemented server-side), replayed anyway in
`automated/views.py` since the mutation never actually applies (the backend
rejects it before touching anything) -- included here only so the baseline
error text is on record for comparison, not because it needs judgment.

**Decision criteria:** only worth a human look if a rerun ever returns
something other than `501 Unimplemented` for this call.

**Baseline (2026-08-03):** `FAIL`, exit 1, all 3 formats: `API request
failed (501): Unimplemented: Metho[d ...]`.

---

<a id="webhooks"></a>

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
