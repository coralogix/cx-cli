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
