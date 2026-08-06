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
