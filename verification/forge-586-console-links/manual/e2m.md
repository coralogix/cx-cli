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
