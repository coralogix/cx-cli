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
