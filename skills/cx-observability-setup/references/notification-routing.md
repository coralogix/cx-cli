# Notification Routing Reference

Complete schema and gotchas for **Notification Center routers** (`cx notifications routers …`).
A router matches incoming notification requests (from alerts, cases, etc.) by **routing labels**,
then evaluates its **rules**. **Every** rule whose **condition** passes delivers to that rule's
**targets** (connector + optional preset) — evaluation does not stop at the first match, so multiple
rules can fire for the same request. `fallbackTargets` are used only when the router matches (by
labels) but **no** rule condition passes; if no router matches at all, the request is dropped.

CLI → API mapping (all under `/mgmt/openapi/5/notifications/notification-center/v1`):

| Command | Method + path |
|---|---|
| `cx notifications routers list` | `GET /routers` |
| `cx notifications routers get <id>` | `GET /routers/{id}` |
| `cx notifications routers create --from-file` | `POST /routers` — **requires `--yes`** |
| `cx notifications routers update --from-file` | `PUT /routers` (full replace; include `id`) — **requires `--yes`** |
| `cx notifications routers delete <id>` | `DELETE /routers/{id}` — **requires `--yes`** |
| `cx notifications routers validate-matcher --from-file` | `POST /routers/matcher/validate` |
| `cx notifications test routing-condition --from-file` | `POST /routers:testCondition` |

---

## Router create/replace payload

The top-level object is wrapped in `router`:

```json
{
  "router": {
    "name": "My router",
    "description": "optional",
    "routingLabels": { "service": "otel-demo", "team": "knowledge-base-demo" },
    "disabled": false,
    "rules": [
      {
        "name": "Rule name",
        "entityType": "ALERTS",
        "condition": "alertDef.name == \"[otel-demo] Checkout Service Errors\"",
        "targets": [
          { "connectorId": "4da1c0f5-7fc5-489e-99ea-b0c9cfee7a3f", "presetId": "optional-preset-id" }
        ]
      }
    ],
    "fallbackTargets": [
      { "entityType": "ALERTS", "target": { "connectorId": "4da1c0f5-..." } }
    ]
  }
}
```

### `router` (GlobalRouter) fields

| Field | Type | Notes |
|---|---|---|
| `name` | string | Router display name. |
| `description` | string | Optional. |
| `routingLabels` | object | `{ service?, team?, environment? }` — see label matching below. |
| `disabled` | bool | Optional; `true` = router does not evaluate. |
| `rules` | array | `RoutingRule`s; **every** rule whose condition passes sends to its targets (no first-match short-circuit). |
| `fallbackTargets` | array | `{ entityType, target: RoutingTarget }` used only when the router matches but **no** rule condition passes. On a **regular (labeled) router the fallback target references a connector only** (no preset). |
| `entityType` | enum | **Reserved for the default router (`router_default`)** — do NOT set it on a regular labeled router. Set `entityType` per-rule instead. |
| `id` | string | Set only on `update` (full replace). |

### `RoutingRule` fields

| Field | Type | Notes |
|---|---|---|
| `name` | string | Rule name. |
| `entityType` | enum | `ALERTS` \| `CASES` \| `TEST_NOTIFICATIONS` \| `ENTITY_TYPE_UNSPECIFIED`. |
| `condition` | string | Tera/expression string, evaluated per event. Empty/omitted = always matches. See conditions below. |
| `targets` | array | One or more `RoutingTarget` (connector + optional preset). |
| `customDetails` | object | Optional `{string: string}`. |

### `RoutingTarget` fields

| Field | Type | Notes |
|---|---|---|
| `connectorId` | string | Connector UUID. Get via `cx notifications connectors list -o json`. |
| `presetId` | string | **Optional** — omit to use the connector's default preset for the entity type. |
| `customDetails` | object | Optional `{string: string}`. |
| `id` | string | Server-assigned; do not set on create. |

---

## ⚠️ Gotcha: `entityType` enum value differs between request and response

- **In create/replace requests** use the bare form: `"ALERTS"`, `"CASES"`, `"TEST_NOTIFICATIONS"`, `"ENTITY_TYPE_UNSPECIFIED"`.
- **In `get`/`list` responses** the same field comes back prefixed: `"ENTITY_TYPE_ALERTS"`.

Sending `"ENTITY_TYPE_ALERTS"` on create fails with:
`400 Bad Request: invalid value for enum field entityType: "ENTITY_TYPE_ALERTS"`.
Do **not** copy a `get` response straight into a create — strip the `ENTITY_TYPE_` prefix first.

---

## Routing labels ↔ alert entity labels

A router only sees a notification request if **all** of the router's `routingLabels` are present on
the source entity. Alerts carry these as `entityLabels` with a `routing.` prefix:

| Router `routingLabels` key | Alert `entityLabels` key |
|---|---|
| `service` | `routing.service` |
| `team` | `routing.team` |
| `environment` | `routing.environment` |

Example: an alert with `entityLabels: { "routing.service": "otel-demo", "routing.team": "knowledge-base-demo" }`
is matched by a router whose `routingLabels` are `{ "service": "otel-demo", "team": "knowledge-base-demo" }`.
Only declare labels the source actually carries — declaring `environment` when the alert has no
`routing.environment` label means the router won't match. Inspect an alert's labels with
`cx alerts get <id> -o json` (look at `alertDef.alertDefProperties.entityLabels`).

Because label matching is automatic, **no edit to the alert definition is required** to route it —
just create a router whose labels match. (An alert can alternatively point at a specific router via
`alertDef.alertDefProperties.notificationGroup.router.id`.)

---

## Conditions (Tera expressions)

`condition` is a boolean expression string (no surrounding `{% if %}`). Common variables:

| Variable | Meaning |
|---|---|
| `alertDef.name` | Alert definition name |
| `alertDef.priority` | Configured priority, e.g. `"P1"` |
| `alert.highestPriority` | Runtime highest priority, e.g. `"P1"`, `"P3"` |
| `alert.status` | Alert status |
| `alert.groups[N].keyValues` | Group-by key/value pairs |
| `_context.entityLabels` | Source entity labels |
| `_context.system.name` | Team/system identifier |

Examples:

```text
alertDef.name == "[otel-demo] Checkout Service Errors"   # route one specific alert
alertDef.priority == "P1"                                 # route by priority
```

Leave `condition` empty to match every request the router's labels select.

Validate before/after creating with `cx notifications test routing-condition --from-file <file>`.

---

## Worked example: route one alert to a Slack connector

Route only `[otel-demo] Checkout Service Errors` to the `knowledge-base-otel-demo` Slack connector,
using its default preset.

1. Find the connector id:
   ```bash
   cx notifications connectors list -o json
   # → "knowledge-base-otel-demo" = 4da1c0f5-7fc5-489e-99ea-b0c9cfee7a3f
   ```
2. Confirm the alert's routing labels:
   ```bash
   cx alerts get <alert-id> -o json | jq '.alertDef.alertDefProperties.entityLabels'
   # → { "routing.service": "otel-demo", "routing.team": "knowledge-base-demo" }
   ```
3. Write `router.json`:
   ```json
   {
     "router": {
       "name": "[otel-demo] Checkout Service Errors",
       "description": "Routes the Checkout Service Errors alert to the knowledge-base-otel-demo Slack connector.",
       "routingLabels": { "service": "otel-demo", "team": "knowledge-base-demo" },
       "rules": [
         {
           "name": "Checkout Service Errors",
           "entityType": "ALERTS",
           "condition": "alertDef.name == \"[otel-demo] Checkout Service Errors\"",
           "targets": [ { "connectorId": "4da1c0f5-7fc5-489e-99ea-b0c9cfee7a3f" } ]
         }
       ]
     }
   }
   ```
4. Create and verify:
   ```bash
   cx notifications routers create --from-file router.json --yes -o json
   cx notifications routers get <new-router-id> -o json   # confirm rules/targets persisted
   ```

---

## Troubleshooting

- **`create`/`update`/`delete` do nothing / prompt** — these are write operations and need `--yes`.
- **`cx notifications connectors entity-types` returns `404 … Connector with id entity-types not found`**
  and **`cx notifications presets list` returns `404 … Preset with id summaries not found`** on some
  backends: these list endpoints use custom-method paths that not every tenant/version routes.
  Work around it by reading ids from what you already have — connector ids from
  `cx notifications connectors list -o json`, and omit `presetId` to use the default preset.
- **`invalid value for enum field entityType`** — you sent the `ENTITY_TYPE_`-prefixed form; use the
  bare form (`ALERTS`) in requests. See the gotcha above.
