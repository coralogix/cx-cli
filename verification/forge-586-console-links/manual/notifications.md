# notifications -- manual verification items

## 1. `notifications test preset`

**Command shape:** unknown -- no working request body was ever found.

**Why it needs judgment:** the original session tried `connectorId` +
`entityType` (accepted) plus various guesses for identifying *which* preset
to render (`presetId`, `preset`, `configOverrides[].id`), all rejected by the
proto validator as unknown fields; omitting an identifier entirely produced a
generic "not a valid identifier" 400. Finding the right field name requires
either reading the service source/proto (explicitly out of scope for the
original probing session) or further live trial-and-error against the API,
which is exactly the kind of guessing an unattended script should not do
silently.

**Decision criteria:** if picking this up again, either (a) find the
request proto/OpenAPI spec for this endpoint and read the correct field name
directly, or (b) resume probing live with `--yes -o json` and small payload
tweaks, one field at a time, recording every attempt (success or 400) via
`record()` the way `step7_slos.py` did for the SLO schema discovery. Once a
working shape is found, promote it into `automated/notifications.py` and this
section can be deleted.

**Baseline (2026-08-03):** `SKIPPED`. Notes: "Could not determine the
required JSON request schema after multiple probing attempts (connectorId +
entityType accepted, but presetId/preset/configOverrides[].id all rejected as
unknown fields by the underlying proto validator, while omitting an
identifier yields a generic 'not a valid identifier' 400); no source/docs
reference was consulted per task constraints (no reading src/). Skipped
rather than continuing to guess."

## Resolved: `notifications test routing-condition` (now automated)

Used to be item 2 here, same situation as `test preset` above -- 6 field-name
guesses (`condition`, `expression`, `matcher`, `routingCondition`,
`entityMatcher`, `conditionExpression`) were all rejected as unknown fields.
A 2026-08-06 manual re-verification pass cracked it: the required field is
`template` (the same field name `test template-render` uses, despite being a
distinct subcommand) -- found via the error message on a bare
`{"entityType": "ALERTS"}` body ("template must not be empty"), never tried
during the original 6-guess session. Working payload:
`{"entityType": "ALERTS", "template": "true"}`, confirmed PASS across all 3
output formats. Promoted into `automated/notifications.py`
(`payloads/notifications_test_routing_condition.json`); no longer a manual
item.

## Not a manual item, but worth knowing

`connectors entity-types` and `connectors entity-subtypes --type SLACK` are
in `automated/notifications.py` even though they `FAIL` every time (404 "not
found", as if the CLI is trying to `GET` a connector whose id is literally
the string `"entity-types"`). That failure is fully pinned to a known cause
and read-only, so it's safe to mechanically replay -- no judgment needed on
each run. Only escalate if a rerun ever shows a *different* error or an
unexpected `200`, since that would mean the underlying routing bug changed
shape or got fixed.

**Baseline (2026-08-03) for that comparison:**
- `connectors entity-types` (all 3 formats) -- FAIL, exit 1: `API request
  failed (404): Not Found: {"reason":"Connector with id entity-types not
  found"}`.
- `connectors entity-subtypes --type SLACK` (all 3 formats) -- FAIL, exit 1:
  `API request failed (404): Not Found: Not Found`.
