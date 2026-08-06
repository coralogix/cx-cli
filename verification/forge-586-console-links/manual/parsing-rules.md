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
