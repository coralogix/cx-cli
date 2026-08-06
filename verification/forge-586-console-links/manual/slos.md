# slos -- manual verification items

## Summary

None currently. Every subcommand actually exercised in the original PR176 run
(`list`, `create`, `get`, `update`, `delete`) ended in a `PASS` using a known
payload shape, and is fully covered by `automated/slos.py`.

The only `FAIL`/non-PASS entries in `OLD_DIR/results/slos.jsonl` were
schema-discovery noise superseded by the final working payload, or a CLI
usage mistake -- neither needs re-judgment on replay:

- `create (setup)` (first real attempt, used the pre-fix `slo_body.json`) --
  FAIL, `exit_code=1`, superseded by "create (attempt 1/2/3)" below and the
  final "create (setup, success)".
- `create (attempt 1: bad field 'serviceName')` -- FAIL: `API request failed
  (400): Bad Request: proto: (line 1:262): unknown field "serviceName"`.
  Top-level `serviceName`/`filters` are not valid fields.
- `create (attempt 2: missing 'sli')` -- FAIL: `ValidationError: "slo.sli" is
  required`. An SLI definition (`requestBasedMetricSli` or
  `windowBasedMetricSli` oneof) is mandatory.
- `create (attempt 3: metric range not [1m])` -- FAIL: `Good events query:
  SLO queries must use a 1-minute range ([1m])...`. PromQL queries inside
  `requestBasedMetricSli.goodEvents`/`totalEvents` must use `[1m]`.
- `update (attempt 1: id as positional arg)` -- FAIL, `exit_code=2`,
  `error: unexpected argument '<id>' found`. `slos update` takes no
  positional id; the id must be embedded in the JSON body.

## If this ever needs re-litigating

If `automated/slos.py`'s `create` step starts failing again with an
`unknown field` / `"slo.sli" is required` / `[1m]` range error, that means
the SLO API schema has changed since 2026-08-03 and the working payload in
`payloads/slos_slo_create.json` needs to be rediscovered the same way the
original session did (trial-and-error against the live `CreateSlo` endpoint,
reading the 400 error's `reason` field each time) -- that rediscovery step
is the one piece of this group that would need LLM/human judgment.
