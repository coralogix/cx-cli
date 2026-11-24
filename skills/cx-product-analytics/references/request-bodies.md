# Product Analytics request bodies

JSON is camelCase. Enum values are bare names such as `EQUAL` or `GROUP_BY_CX_USER_ID`.
64-bit numbers may be sent as numbers or strings and come back as strings
(`"sessionCount": "3966"`). Fields left at their default are omitted from responses.
Flags on the command line override the same fields in a `--from-file` body. Unknown
fields are rejected with 400.

## Time ranges

Queries take their time range from `--start` / `--end`, sent as Unix **seconds** in
`fromTimestamp` / `toTimestamp` (inside `query` for funnels). Do not put them in a query
file: the flags always replace them.

A saved object's own `timeRange` (required in `create` and `update` bodies) is exactly
one of:

```json
{"relativeMinutes": "1440"}
{"fixed": {"fromTimestamp": "1763251200", "toTimestamp": "1763337600"}}
{"calendar": "CALENDAR_WINDOW_TODAY"}
```

`fixed` is in Unix seconds. `calendar` is `CALENDAR_WINDOW_TODAY` or
`CALENDAR_WINDOW_YESTERDAY`. `conversionWindowSeconds` (funnels) is in seconds too.

## Filters (`filters`)

A list, combined with AND. Each item is a `field` and exactly one condition:

```json
{"field": "PLATFORM", "stringCondition": {"values": ["browser"], "operator": "EQUAL"}}
{"field": "ERROR_COUNT", "numericCondition": {"values": ["1"], "operator": "MIN"}}
{"field": "SCROLL_DEPTH_PERCENTAGE", "numericCondition": {"values": ["50", "100"], "operator": "RANGE"}}
{"field": "HAS_SESSION_REPLAY", "boolCondition": true}
```

Numeric operators: `MIN` (>=, one value), `MAX` (<=, one value), `RANGE` (two values,
both included).

String operators: `EQUAL` (default), `NOT_EQUAL`, `CONTAINS`, `NOT_CONTAINS`,
`START_WITH`, `NOT_START_WITH`, `END_WITH`, `NOT_END_WITH`. `values` matches any of them.

String fields: `URL_SCHEMA`, `URL_SUBDOMAIN`, `URL_DOMAIN`, `URL_HOST`, `URL_PATH`,
`URL_TEMPLATE`, `REFERRER_SCHEMA`, `REFERRER_SUBDOMAIN`, `REFERRER_DOMAIN`,
`REFERRER_HOST`, `REFERRER_PATH`, `REFERRER_URL`, `SESSION_ID`, `USER_ID`, `USER_EMAIL`,
`USER_NAME`, `CX_USER_ID`, `ACCOUNT_ID`, `ACCOUNT_NAME`, `VIEW_ID`, `PLATFORM`,
`ENVIRONMENT`, `APP_VERSION`, `CX_SDK_VERSION`, `BROWSER_NAME`, `BROWSER_VERSION`,
`BROWSER_MAJOR_VERSION`, `OS`, `OS_MAJOR_VERSION`, `OS_VERSION`, `DEVICE_TYPE`,
`DEVICE_NAME`, `DEVICE_BRAND`, `DEVICE_MODEL`, `DEVICE_ARCHITECTURE`, `COUNTRY_NAME`,
`CITY_NAME`, `CONTINENT_NAME`, `IP`, `USER_AGENT`, `APP_NAME`,
`CLICKS_ELEMENT_FINGERPRINT`.

Numeric fields: `ERROR_COUNT`, `ACTION_COUNT`, `LONG_TASK_COUNT`, `CRASH_COUNT`,
`RESOURCE_COUNT`, `SCROLL_DEPTH_PERCENTAGE`, `VIEWPORT_WIDTH`, `VIEWPORT_HEIGHT`,
`RAGE_CLICK_COUNT`, `DEAD_CLICK_COUNT`.

Boolean fields: `HAS_SESSION_REPLAY`, `HAS_SCREENSHOT`, `IS_RAGE_CLICK`,
`IS_DEAD_CLICK`.

`cx product-analytics filter-values --app <app> --field <field>` lists the values a
field takes. `views list` also accepts `"pageFilters": [...]` (page conditions, below)
and `actions list` accepts `"actionFilters": [...]` (action conditions).

## Funnel query (`funnels query` / `matching-users` `--from-file`)

```json
{
  "query": {
    "appNames": ["web-shop"],
    "steps": [
      {"stepId": "s1", "label": "Viewed cart",
       "conditions": [{"page": {"match": {"values": ["/cart"], "operator": "EQUAL"}}}]},
      {"stepId": "s2", "label": "Clicked pay",
       "conditions": [{"action": {"specificWithTargetElement": {
          "eventName": {"values": ["click"]},
          "targetElement": {"values": ["pay-now-button"]}}},
        "filters": [{"field": "URL_PATH", "stringCondition": {"values": ["checkout"], "operator": "CONTAINS"}}]}]}
    ],
    "filters": [],
    "conversionWindowSeconds": "3600"
  },
  "groupBy": "GROUP_BY_CX_USER_ID",
  "breakdown": {"field": "PLATFORM", "limit": 5}
}
```

- A step is reached when any of its `conditions` matches (OR). A condition holds a
  `page` and/or an `action`, plus optional per-condition `filters`.
- `page` is `{"any": {}}` or `{"match": <string condition>}` on the view name.
- `action` is `{"any": {}}`, `{"specificEvent": <string condition>}` (event name such as
  `click`) or `{"specificWithTargetElement": {"eventName": ..., "targetElement": ...}}`.
- A condition with both `page` and `action` needs `"combinator": "OR"`.
- `groupBy` (or `--group-by`): `GROUP_BY_SESSION_ID` (default), `GROUP_BY_CX_USER_ID`,
  `GROUP_BY_ACCOUNT_ID`.
- `breakdown.field` is a session-level filter field (`PLATFORM`, `BROWSER_NAME`, `OS`,
  `COUNTRY_NAME`, `ACCOUNT_NAME`, ...); `limit` is 1 to 1000. Each group in the
  result carries its `groupValue`.
- `matching-users` uses only `query` from the file; the CLI adds `mode`, `startStep`,
  `endStep` (the number of steps), `limit` and `offset`.

Result: `{"groups": [{"groupValue": "...", "steps": [{"stepNumber": 1, "sessionCount": "...",
"conversionRate": 0.31, "avgTimeFromPrevMs": "51730"}]}]}`. `conversionRate` is relative to
step 1 (0 to 1); `sessionCount` counts the `groupBy` unit.

## Other query files

`views list`, `actions list`, `filter-values`, `pathways query`, `heatmaps query` and
`heatmaps scroll-map` take `--from-file` for extra fields, mainly `filters`. The flags
set the app, view, time range and options.

## Create bodies (`create --from-file`)

The file is the object itself, not wrapped in a key.

Funnel (note `appNames`, a one-item list):
```json
{"appNames": ["web-shop"], "name": "Checkout funnel", "steps": [...], "filters": [],
 "groupBy": "GROUP_BY_CX_USER_ID", "conversionWindowSeconds": "3600",
 "timeRange": {"relativeMinutes": "1440"}, "breakdown": {"field": "PLATFORM", "limit": 5}}
```

Pathway (`pagesCount` and `limit` are required and positive; `mode` is
`PATHWAYS_MODE_FORWARD`, the default, or `PATHWAYS_MODE_BACKWARD`):
```json
{"appName": "web-shop", "name": "After cart", "pageName": "/cart", "pagesCount": 5,
 "limit": 20, "mode": "PATHWAYS_MODE_FORWARD", "filters": [],
 "timeRange": {"relativeMinutes": "1440"}}
```

Heatmap (`mode` is `TOP_ELEMENTS`, `CLICK_MAP` or `SCROLL_MAP`; `actionNameFilter` is
`ALL_CLICKS`, the default, `NAMED_ONLY` or `UNNAMED_ONLY`):
```json
{"appName": "web-shop", "name": "Checkout clicks", "viewName": "/checkout",
 "mode": "CLICK_MAP", "actionNameFilter": "ALL_CLICKS", "filters": [],
 "timeRange": {"calendar": "CALENDAR_WINDOW_TODAY"}}
```

`create` returns `{"id": "51"}`.

## Update bodies (`update <id> --from-file`)

The same fields as create, without the app (`appNames` / `appName`): the file replaces
the whole saved object, and anything missing is cleared. `id`, `appName` and `createdAt`
are dropped by the CLI, so an edited `get -o json` output works as is.
