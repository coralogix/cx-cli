# Product Analytics request bodies

JSON is camelCase. Enum values are the full names, such as `FUNNEL_MODE_USERS`.
Flags on the command line override the same fields in a `--from-file` body.

## Time range (`timeRange`)

Queries take their time range from `--start` / `--end`, sent as
`{"fixed": {"fromTimestampMs": ..., "toTimestampMs": ...}}` (from inclusive, to
exclusive). Do not put `timeRange` in a query file.

A saved object's own `timeRange` (in `create` and `update` bodies) is exactly one of:

```json
{"relativeMinutes": "1440"}
{"fixed": {"fromTimestampMs": "1763251200000", "toTimestampMs": "1763337600000"}}
{"calendar": "CALENDAR_WINDOW_TODAY"}
```

`calendar` is `CALENDAR_WINDOW_TODAY` or `CALENDAR_WINDOW_YESTERDAY`.

## Filters (`filters`)

A list, combined with AND. Each item is exactly one of:

```json
{"stringFilter": {"field": "STRING_FILTER_FIELD_PLATFORM",
                  "condition": {"values": ["web"], "operator": "STRING_CONDITION_OPERATOR_EQUAL"}}}
{"numericFilter": {"field": "NUMERIC_FILTER_FIELD_ERROR_COUNT", "condition": {"min": "1"}}}
{"numericFilter": {"field": "NUMERIC_FILTER_FIELD_SCROLL_DEPTH_PERCENTAGE",
                   "condition": {"range": {"lower": "50", "upper": "100"}}}}
{"boolFilter": {"field": "BOOL_FILTER_FIELD_HAS_SESSION_REPLAY", "value": true}}
```

A numeric condition is exactly one of `min` (>=), `max` (<=) or `range` (both
included).

String operators: `STRING_CONDITION_OPERATOR_` + `EQUAL`, `NOT_EQUAL`, `CONTAINS`,
`NOT_CONTAINS`, `START_WITH`, `NOT_START_WITH`, `END_WITH`, `NOT_END_WITH`.

String fields (`STRING_FILTER_FIELD_` +): `URL_SCHEMA`, `URL_SUBDOMAIN`, `URL_DOMAIN`,
`URL_HOST`, `URL_PATH`, `URL_TEMPLATE`, `REFERRER_SCHEMA`, `REFERRER_SUBDOMAIN`,
`REFERRER_DOMAIN`, `REFERRER_HOST`, `REFERRER_PATH`, `REFERRER_URL`, `SESSION_ID`,
`USER_ID`, `USER_EMAIL`, `USER_NAME`, `CX_USER_ID`, `ACCOUNT_ID`, `ACCOUNT_NAME`,
`VIEW_ID`, `PLATFORM`, `ENVIRONMENT`, `APP_VERSION`, `CX_SDK_VERSION`, `BROWSER_NAME`,
`BROWSER_VERSION`, `BROWSER_MAJOR_VERSION`, `OS`, `OS_MAJOR_VERSION`, `OS_VERSION`,
`DEVICE_TYPE`, `DEVICE_NAME`, `DEVICE_BRAND`, `DEVICE_MODEL`, `DEVICE_ARCHITECTURE`,
`COUNTRY_NAME`, `CITY_NAME`, `CONTINENT_NAME`, `IP`, `USER_AGENT`, `APP_NAME`,
`CLICKS_ELEMENT_FINGERPRINT`.

Numeric fields (`NUMERIC_FILTER_FIELD_` +): `ERROR_COUNT`, `ACTION_COUNT`,
`LONG_TASK_COUNT`, `CRASH_COUNT`, `RESOURCE_COUNT`, `SCROLL_DEPTH_PERCENTAGE`,
`VIEWPORT_WIDTH`, `VIEWPORT_HEIGHT`, `RAGE_CLICK_COUNT`, `DEAD_CLICK_COUNT`.

Boolean fields (`BOOL_FILTER_FIELD_` +): `HAS_SESSION_REPLAY`, `HAS_SCREENSHOT`,
`IS_RAGE_CLICK`, `IS_DEAD_CLICK`.

## Funnel definition (`definition`)

```json
{
  "mode": "FUNNEL_MODE_USERS",
  "conversionWindow": "3600s",
  "steps": [
    {"stepId": "s1", "label": "Viewed cart",
     "conditions": [{"view": {"name": {"values": ["/cart"], "operator": "STRING_CONDITION_OPERATOR_EQUAL"}}}]},
    {"stepId": "s2", "label": "Clicked pay",
     "conditions": [{"action": {
        "event": {"values": ["click"], "operator": "STRING_CONDITION_OPERATOR_EQUAL"},
        "element": {"values": ["pay-now-button"], "operator": "STRING_CONDITION_OPERATOR_EQUAL"}},
       "filters": []}]}
  ]
}
```

- `mode`: `FUNNEL_MODE_USERS`, `FUNNEL_MODE_SESSIONS` (every step in one session) or
  `FUNNEL_MODE_ACCOUNTS`.
- `conversionWindow`: whole seconds such as `"3600s"`; omit for no window.
- A step is reached when any of its conditions matches. A condition is exactly one of
  `view` (page name; empty matches any page) or `action` (event, optionally on an
  element; `element` requires `event`). Each condition can carry its own `filters`.
- Step IDs must be unique; the UI uses `s1`, `s2`, ...

## Breakdown (`breakdown`)

```json
{"field": "BREAKDOWN_FIELD_PLATFORM", "limit": 5}
```

Fields (`BREAKDOWN_FIELD_` +): `ACCOUNT_ID`, `ACCOUNT_NAME`, `PLATFORM`, `ENVIRONMENT`,
`APP_VERSION`, `CX_SDK_VERSION`, `BROWSER_NAME`, `OS`, `OS_VERSION`, `DEVICE_TYPE`,
`COUNTRY_NAME`, `CITY_NAME`, `CONTINENT_NAME`. `limit` is 1 to 1000.

For `funnels matches` on one group of a broken-down funnel, add
`"group": {"field": "BREAKDOWN_FIELD_PLATFORM", "value": "web"}` and the same
`breakdown` as the query.

## Query bodies (`--from-file` for `query`, `matches`, `views list`, `events`)

| Command | Saved form | Inline form |
|---|---|---|
| `funnels query` / `matches` | `funnelId` (positional ID) | `appName` + `definition` |
| `pathways query` | `pathwayId` (positional ID) | `appName` + `view` |
| `heatmaps query` | `heatmapId` (positional ID) | `appName` + `view` |

Other fields: `filters`, `breakdown` (funnels), `mode` (funnels, saved
only), `direction` / `pagesCount` / `topN` (pathways), `actionNameFilter` / `topN`
(heatmaps). `actionNameFilter` is `ACTION_NAME_FILTER_ALL_CLICKS`, `_NAMED_ONLY` or
`_UNNAMED_ONLY`.

## Create bodies (`create --from-file`)

The file is the object itself, not wrapped in a key.

Funnel:
```json
{"appName": "web-shop", "name": "Checkout funnel", "definition": {...},
 "filters": [], "timeRange": {"relativeMinutes": "1440"},
 "breakdown": {"field": "BREAKDOWN_FIELD_PLATFORM", "limit": 5}}
```

Pathway (`pagesCount` default 5, `topN` default 20, `direction` default after):
```json
{"appName": "web-shop", "name": "After cart", "view": "/cart",
 "direction": "PATHWAY_DIRECTION_AFTER", "pagesCount": 5, "topN": 20,
 "filters": [], "timeRange": {"relativeMinutes": "1440"}}
```

Heatmap (`mode` is `HEATMAP_MODE_TOP_ELEMENTS`, `HEATMAP_MODE_CLICK_MAP` or
`HEATMAP_MODE_SCROLL_MAP`):
```json
{"appName": "web-shop", "name": "Checkout clicks", "viewName": "/checkout",
 "mode": "HEATMAP_MODE_CLICK_MAP", "actionNameFilter": "ACTION_NAME_FILTER_ALL_CLICKS",
 "filters": [], "timeRange": {"calendar": "CALENDAR_WINDOW_TODAY"}}
```

## Update masks

Updatable fields:

| Kind | Fields |
|---|---|
| Funnel | `name`, `definition`, `filters`, `timeRange`, `breakdown` |
| Pathway | `name`, `view`, `direction`, `pagesCount`, `topN`, `timeRange`, `filters` |
| Heatmap | `viewName`, `name`, `filters`, `snapshotId`, `timeRange`, `mode`, `actionNameFilter` |

Only top-level fields: nested paths such as `definition.steps` return 400. `appName`
cannot change.
