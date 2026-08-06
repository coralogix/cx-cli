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
