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
