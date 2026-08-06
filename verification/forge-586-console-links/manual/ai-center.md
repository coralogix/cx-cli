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
