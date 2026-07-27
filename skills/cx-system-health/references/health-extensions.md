# Health: Extensions data requirements  *(STUB — for the owning PM to complete)*

> **Status:** scaffold. Complete from `contributing/health-reference-md-template.md`. Extensions =
> Coralogix quick-start integration packages (dashboards + alerts + parsing for a specific tech, e.g.
> nginx, MySQL, a cloud service). An extension only delivers value if the telemetry it expects is
> actually arriving with the right shape.

Check whether an installed extension has the data it needs to populate its dashboards/alerts.

## When to use this reference

"Does my `<tech>` extension have the data it needs", "why is the `<extension>` dashboard empty", "is
the integration for `<tech>` actually working".

## Conditions & checks (to complete)

| # | Condition | Kind | Check | Fail → |
|---|---|---|---|---|
| 1 | The extension is installed/configured | Presence | `cx integrations list` | **missing** → install (route to setup) |
| 2 | The signal the extension parses is arriving | Presence | `cx logs/metrics/spans` for the extension's expected source | **missing** → `cx-onboarding` |
| 3 | Data matches the extension's expected fields/format | Completeness | inspect sample vs the extension's schema | **degraded** → fix parsing/attributes |
| 4 | *TODO (PM):* extension-specific quality conditions | Quality | | |

## Verdict → remediation

- **missing** → route to onboarding for the source signal, or install the extension.
- **degraded** → the data doesn't match what the extension parses → fix at the instrumentation/parsing
  layer (route to `cx-onboarding`; parsing rules are `cx-data-pipeline`).

## TODO (owning PM)

Fill the extension-specific conditions, the exact `cx integrations` output to expect, and the mapping
from "extension X" → "signals + fields it requires". Consider generating this per-extension from the
extension catalog rather than hand-authoring each.

## Sources / evidence

Scaffold (2026-07). `cx integrations` manages integrations/extensions/contextual data.
