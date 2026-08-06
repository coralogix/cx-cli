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
