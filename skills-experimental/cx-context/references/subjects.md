# Subjects

Draft: the list of types and the ways to find their IDs will grow.

A subject is `type` or `type:value`. Lookups match it exactly as a string, so an entry saved under one spelling is invisible under another. Before saving, run `cx context subjects` to see the types this team already uses.

## Coralogix entities

These types name one Coralogix entity. Use the ID below as the value, never a name or a label. `cx` refuses a write whose value cannot be that ID.

| Type | What it is | Value | How to get it |
|---|---|---|---|
| `alert` | Alert definition | Its definition ID, a UUID. Not the alert's name, not the name in a config file, not a version or event ID | `cx alerts list --name <text>`, then the `id` of the definition |
| `case` | Case | Its UUID. Not the readable `CASE-123` | `cx cases get CASE-123` shows the UUID |
| `dataset` | Dataset | `dataspace/dataset`, for example `default/logs` or `default/enrichments/ip_lookup` | The `source` of a DataPrime query |
| `dashboard` | Dashboard | Its ID | `cx dashboards search <text>` or `cx dashboards catalog` |
| `slo` | SLO | Its UUID | `cx slos list` |

A type alone (`alert`, `dataset`) is for knowledge about all entities of that type.

## Free subjects

Anything else has no Coralogix ID. The type is a short lower-case word and the value is the name as observed:

- `service:checkout`
- `host:i-0abc1234def567890`
- `ip:10.2.0.7`
- `user:<login>`

Reuse a type the team already uses (`cx context subjects`) before coining a new one: `host` and `hostname` do not match each other.

For a service, reuse the spelling the team has already saved under. If there is none, prefer the service name as it appears in spans or in the service catalog (`cx service-catalog entities service`); otherwise use the component name as observed. A log application or subsystem name, or an alert's name, is not necessarily the same service. Keep case and punctuation. Put other confirmed names of the same thing in the body, where search finds them.
