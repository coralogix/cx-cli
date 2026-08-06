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
