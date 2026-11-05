# Datadog Browser SDK → Coralogix Browser RUM SDK mapping

Verified against: @datadog/browser-* 7.17.0, @coralogix/browser 4.4.0 (2026-11-05)

Sources: Datadog `RumInitConfiguration` / `InitConfiguration` / `RumPublicApi` / `LogsInitConfiguration` / `Logger` (`src/` of `@datadog/browser-rum-core`, `-core`, `-logs`, `-rum-react` 7.17.0); `@datadog/browser-rum-angular|vue|nextjs` 7.17.0 READMEs; Coralogix `src/types.d.ts`, `src/session/session.model.d.ts`, README and bundled JS of `@coralogix/browser` 4.4.0. Rows marked "from source" describe behavior read in the Coralogix bundle, not in its README.

Rules for the agent applying this file:
- Use only the Coralogix APIs named here. Never call a Datadog name on `CoralogixRum`.
- "Approximation" means the data lands differently; tell the user. "No equivalent" means delete the call/option and list it in the migration report.
- The `CoralogixRum` API is a no-op (debug log only) before `init()`. There is no `onReady` queue.
- Init details (required fields, key type, regions): see `rum-browser-init.md`.

## 1. Packages and loading

| Datadog | Coralogix | Notes |
|---|---|---|
| `@datadog/browser-rum` | `@coralogix/browser` | `import { CoralogixRum } from '@coralogix/browser'`. |
| `@datadog/browser-rum-slim` (no replay) | `@coralogix/browser` | Recorder is loaded via dynamic import only when `sessionRecordingConfig.enable` is true and the session is sampled in (from source). |
| `@datadog/browser-logs` | `@coralogix/browser` | Logs are part of the same SDK (`CoralogixRum.info/warn/error/...`). Merge the two DD inits into one `CoralogixRum.init`. |
| `@datadog/browser-rum-react`, `-rum-angular`, `-rum-vue`, `-rum-nextjs` | none | Coralogix publishes no per-framework browser packages (npm `@coralogix/*` scope checked: only `browser`, build plugins `webpack-plugin`/`vite-plugin`/`esbuild-plugin`, `rum-cli`, mobile/other SDKs). See §9. |
| CDN `https://www.datadoghq-browser-agent.com/<site>/v7/datadog-rum.js` (+ `datadog-logs.js`) | `https://cdn.rum-ingress-coralogix.com/coralogix/browser/4.4.0/coralogix-browser-sdk.js` (ES5: `coralogix-browser-sdk.es5.js`) | Pin the version; `latest` is not maintained. |
| `window.DD_RUM.onReady(() => window.DD_RUM.init({...}))` (async snippet) | `<script src=".../coralogix-browser-sdk.js"></script>` then `window.CoralogixRum.init({...})` | No `onReady`/command queue. Load the script synchronously before any `window.CoralogixRum` call, or guard with `window.CoralogixRum?.`. |
| `window.DD_LOGS` | `window.CoralogixRum` | Same global for logs. |

## 2. Init options

| Datadog | Coralogix | Notes |
|---|---|---|
| `applicationId` + `clientToken` | `public_key` | Different credential: a Coralogix RUM key (SendRUMData preset), from the user's Coralogix team. Ask the user; never reuse the DD token. Coralogix has no application-ID concept: the app is identified by `application` (string). Init aborts (console warn) without `public_key` unless `proxyUrl` is set (from source). |
| `service` | `application` | Required in Coralogix (init aborts if missing). DD defaults `service` to `applicationId`; if DD had no `service`, ask the user for an app name. Must match the `-a` used for source-map uploads. |
| `version` | `version` | Required in Coralogix (init aborts if missing); optional in DD. If DD had none, wire it to the build's release version. Must match source-map upload `-v`. |
| `env` | `environment` | Optional string. |
| `site` | `coralogixDomain` (`EU1` `EU2` `US1` `US2` `US3` `AP1` `AP2` `AP3`) or `coralogixDomainUrl` | Do not derive from the DD site. The value is the region of the user's Coralogix team; ask. See table below. |
| `proxy: 'https://p/x'` | `proxyUrl: 'https://p/x'` | Proxy server code must change. DD appends `ddforward=<url-encoded intake path + query>`; Coralogix appends `cxforward=<url-encoded full Coralogix ingress URL>` (e.g. `https://ingress.eu1.rum-ingress-coralogix.com/browser/v1beta/logs`) and POSTs. The proxy must forward to the decoded `cxforward` (restrict to `*.rum-ingress-coralogix.com`). `ignoreProxyUrlParams: true` drops `cxforward`. Coralogix adds `proxyUrl` to `ignoreUrls` automatically (from source). |
| `proxy: (opts) => url` (function) | none | No function form. Use a static `proxyUrl` and route on `cxforward`. |
| `trackingConsent: 'not-granted'` / `setTrackingConsent()` | none | No consent API. Pattern: call `CoralogixRum.init()` only after consent is granted; on revocation call `CoralogixRum.shutdown()`. UNVERIFIED: whether `init()` works again after `shutdown()` in the same page. |
| `allowedTrackingOrigins` (browser extensions) | none | Remove. |
| `trackUserInteractions` | `instrumentations.interactions` | Default true in both. |
| `trackResources` | `instrumentations.resources` (+ `fetch`, `xhr`) | DD resources include fetch/XHR; Coralogix `resources` covers static assets only (excludes fetch/xhr initiators, from source). `fetch: false`/`xhr: false` also stop trace header injection. |
| `trackLongTasks` | `instrumentations.long_tasks` | Default true in both. |
| (always on) web vitals, errors | `instrumentations.web_vitals`, `instrumentations.errors` | Default true. `errors` captures window errors, unhandled rejections and `console.error` (from source). |
| `actionNameAttribute: 'data-x'` | `actionNameAttribute: 'data-x'` | Same semantics (nearest element carrying it names the click). Coralogix accepts one name matching `[A-Za-z_][A-Za-z0-9_-]*`. |
| `data-dd-action-name="..."` markup | `data-cx-action-name="..."`, or `actionNameAttribute: 'data-dd-action-name'` | Setting `actionNameAttribute` keeps markup unchanged. Only one custom attribute: if DD used both a custom `actionNameAttribute` and `data-dd-action-name`, keep one and rewrite the other. |
| `data-dd-ignore-frustration` | `data-cx-action-ignore` | Approximation: Coralogix still reports the click with `is_ignored: true`; applies to the exact node only, not descendants. |
| `enablePrivacyForActionName` | `maskClass` / `maskInputTypes` | See §7. |
| `trackViewsManually` + `startView(name)` | none | Coralogix derives the view from the URL (`page_context.page_url_blueprint`, `page_fragments`; `CoralogixRum.getViewName()` returns the current one). To get route-pattern names use `urlBlueprinters.pageUrlBlueprinters` (built-in already replaces UUID and all-digit segments with `{id}`). |
| `setViewName(name)` | none | Approximation: `pageUrlBlueprinters` or a label (`setLabels({...getLabels(), view_name: name})`). |
| `sessionPersistence` (`cookie`/`local-storage`/`memory`), `trackSessionAcrossSubdomains`, `useSecureSessionCookie`, `usePartitionedCrossSiteSessionCookie` | none | For multi-page apps set `sessionConfig.keepSessionAfterReload: true`. Coralogix rotates sessions after 15 min idle or 1 h. |
| `storeContextsAcrossPages` | none | Re-apply `setUserContext` / `setLabels` on every page load. |
| `trackAnonymousUser` | none (always on) | Coralogix computes unique users from a stored fingerprint; no opt-out option. `collectIPData: false` disables IP/geo collection (separate concern). |
| `silentMultipleInit` | none | Guard with `if (!CoralogixRum.isInited)`. |
| `enableExperimentalFeatures` | none | Coralogix experimental flags are different features (`trackSoftNavigations`, `memoryUsageConfig`); do not map. |
| `telemetrySampleRate`, `telemetryConfigurationSampleRate`, `telemetryUsageSampleRate` | none | Remove. (`debug: true` is console logging only.) |
| `allowUntrustedEvents`, `subdomain`, `remoteConfiguration*`, `replica`, `betaEnableViewUpdates` | none | Remove. |
| `init(config, { user })` (InitContext) | `init({ ..., user_context: {...} })` | See §4 for shape. |

DD `site` values, for asking the user only (do not auto-map; Coralogix region is the user's Coralogix team region):

| DD `site` | DD name | Location |
|---|---|---|
| `datadoghq.com` (default) | US1 | US |
| `us3.datadoghq.com` | US3 | US |
| `us5.datadoghq.com` | US5 | US |
| `datadoghq.eu` | EU1 | EU (Germany) |
| `uk1.datadoghq.com` | UK1 | UK |
| `ap1.datadoghq.com` | AP1 | Japan |
| `ap2.datadoghq.com` | AP2 | Australia |
| `ddog-gov.com` | US1-FED | US |
| `us2.ddog-gov.com` | US2-FED | US |

## 3. Sampling

| Datadog | Coralogix | Notes |
|---|---|---|
| `sessionSampleRate` (0–100, default 100) | `sessionConfig.sessionSampleRate` (0–100, default 100) | Same scale; copy the value. Coralogix also has a top-level `sessionSampleRate`; both are drawn independently (effective = product, from source). Set only one: `sessionConfig.sessionSampleRate`. |
| `sessionReplaySampleRate` (0–100, default **0**) | `sessionRecordingConfig.sessionRecordingSampleRate` (0–100, default **100** when recording is enabled) | Both are a percentage of tracked sessions, so copy the value. Effective share of all sessions = session × replay / 100 in both (e.g. 50 and 20 → 10 %). If DD omitted `sessionReplaySampleRate` (0), do not enable `sessionRecordingConfig`. If set, always write `sessionRecordingSampleRate` explicitly. |
| `startSessionReplayRecordingManually: true` | `sessionRecordingConfig.autoStartSessionRecording: false` | Recording still needs `enable: true`. |
| `traceSampleRate`, `traceContextInjection` | none | Coralogix has no per-request trace sampling option. |
| `profilingSampleRate` | none | No browser profiling in `@coralogix/browser`. |
| (none) | `sessionConfig.alwaysTrackSessionsWithErrors`, `onlyWithErrorConfig`, `excludeFromSampling` | Coralogix-only options; offer, do not add silently. |

## 4. User, account and context

| Datadog | Coralogix | Notes |
|---|---|---|
| `setUser({ id, name, email, ...rest })` | `setUserContext({ user_id: id, user_name: name ?? '', user_email: email, user_metadata: { ...rest } })` | Coralogix types require `user_id` and `user_name` (strings); DD requires only `id`. If DD has no name, pass `''` (do not invent PII). `setUserContext` replaces the whole user context (from source). |
| `setUserProperty(k, v)` | `const u = CoralogixRum.getUserContext(); CoralogixRum.setUserContext({ ...u, user_metadata: { ...u?.user_metadata, [k]: v } })` | Read-modify-write; `id`/`name`/`email` keys go to the top-level fields instead. |
| `removeUserProperty(k)` | read-modify-write without `k` | |
| `clearUser()` | `setUserContext({ user_id: '', user_name: '' })` | Matches the SDK's default user context (from source). On logout also consider `CoralogixRum.createNewSession()`. |
| `getUser()` | `getUserContext()` | |
| `setAccount({ id, name, ...rest })` | `setUserContext({ ...getUserContext(), account_id: id, account_name: name })` | `account_id`/`account_name` are fields of `UserContextConfig`. Extra account props: put under `user_metadata` (approximation). |
| `setAccountProperty` / `removeAccountProperty` / `clearAccount` / `getAccount` | read-modify-write of `account_*` via `getUserContext`/`setUserContext` | |
| `setGlobalContext(ctx)` | `setLabels(ctx)` | `setLabels` **replaces** all custom labels (from source). Labels go to `labels` on every event. |
| `setGlobalContextProperty(k, v)` | `setLabels({ ...CoralogixRum.getLabels(), [k]: v })` | Merge manually. |
| `removeGlobalContextProperty(k)` | `const { [k]: _, ...rest } = CoralogixRum.getLabels(); CoralogixRum.setLabels(rest)` | |
| `clearGlobalContext()` | `setLabels({})` | |
| `getGlobalContext()` | `getLabels()` | |
| Logs SDK `setGlobalContext*` / `setUser*` | same as above | One shared context in Coralogix. |
| `setViewContext(ctx)` / `setViewContextProperty` / `getViewContext` | none | Approximation: set labels on route change and remove them on the next one. |
| (none) | `labelProviders`, `setApplicationContext({ application, version })` | Coralogix-only. |

## 5. Manual capture

| Datadog | Coralogix | Notes |
|---|---|---|
| `addError(error, context)` | `captureError(err, context, labels?)` | Coralogix requires an `Error`: `captureError(error instanceof Error ? error : new Error(String(error)), context)`. `context` → `error_context.error_custom_data`. |
| `addAction(name, context)` | Approximation: `CoralogixRum.info(name, context, { action_name: name })` | No custom-action API; this sends a custom log event. Label key is a suggestion; keep it consistent. |
| `startAction(name)` / `stopAction(name)` | Approximation: `startTimeMeasure(name, labels)` / `endTimeMeasure(name)` | Sends a custom measurement (duration), not an action. No error/resource association. |
| `addTiming(name, time?)` | `addTiming(name, duration?)` | Both default to "now minus current view/page start". DD stores in `view.custom_timings`; Coralogix sends a custom measurement event. DD `time` is a relative timestamp; Coralogix `duration` is the value sent as-is. |
| `setViewLoadingTime()` | Approximation: `addTiming('view_loading_time')` | |
| `addDurationVital(name, { startTime, duration, context })` | Approximation: `sendCustomMeasurement(name, duration)` | `startTime`, `context`, `description` are dropped. |
| `startDurationVital(name, { context, vitalKey })` / `stopDurationVital(name, { vitalKey })` | `startTimeMeasure(key, labels)` / `endTimeMeasure(key)` | Coralogix keys by name only: a second start for a running key is ignored. For `vitalKey`, use `` `${name}:${vitalKey}` `` as the key. Labels only at start. |
| `startOperation` / `succeedOperation` / `failOperation(name, reason)` | Approximation: `startTimeMeasure` / `endTimeMeasure`, plus `CoralogixRum.error(name, { reason })` on failure | |
| `startResource(url)` / `stopResource(url)` | Approximation: custom spans (`getCustomTracer()`, needs `traceParentInHeader.enabled`) | |
| `addFeatureFlagEvaluation(key, value)` | Approximation: `setLabels({ ...getLabels(), [`ff_${key}`]: value })` | Applies to all later events, not only the current view. |
| `getSessionReplayLink()` | none | `getSessionId()` returns the Coralogix session id. |
| `stopSession()` | Approximation: `createNewSession()` | Coralogix starts the new session immediately; DD at next interaction. |
| `getInitConfiguration()`, `getInternalContext()` | none | |
| `datadogLogs.logger.<level>(msg, ctx, error?)` | `CoralogixRum.<level>(msg, data?, labels?)` | Severity mapping below. If DD passes an `Error` third arg, also call `captureError(error, ctx)`. Needs `instrumentations.custom` (default true). Use `warn`, not `warning` (the README example `CoralogixRum.warning` is not in `types.d.ts`). |
| `logger.log(msg, ctx, status)` | `CoralogixRum.log(CoralogixLogSeverity.X, msg, data)` | `import { CoralogixLogSeverity } from '@coralogix/browser'`. |
| `createLogger(name, conf)` / `getLogger(name)` | none | Approximation: pass `{ logger: name }` as `labels` on each call. |
| `logger.setContext` / `setContextProperty` / `addTag` | none | Merge into each call's `data`, or use `setLabels`. |
| `logger.setLevel`, `logger.setHandler(...)` | none | Filter at call site or in `beforeSend`. |
| `forwardErrorsToLogs: true` | `instrumentations.errors: true` | Errors become RUM error events. |
| `forwardConsoleLogs: ['error']` | `instrumentations.errors` (captures `console.error`) | Other console levels: no equivalent as events. `sessionRecordingConfig.recordConsoleEvents: true` puts console output in the replay only. |
| `forwardReports` (CSP, deprecation, intervention) | none | |

DD log status → `CoralogixLogSeverity` (Debug 1, Verbose 2, Info 3, Warn 4, Error 5, Critical 6):

| DD | Coralogix | DD | Coralogix |
|---|---|---|---|
| `ok`, `debug` | `debug` | `error` | `error` |
| `info`, `notice` | `info` | `critical`, `alert`, `emerg` | `critical` |
| `warn` | `warn` | | |

## 6. Filtering

| Datadog | Coralogix | Notes |
|---|---|---|
| `beforeSend(event, context) => boolean` (mutate in place, `false` drops) | `beforeSend(event: EditableCxRumEvent) => event \| null` | Return the event to keep, `null` to drop. No second `context` argument (no access to original `Error`, XHR, or response). |
| `event.type` (`view`/`action`/`error`/`resource`/`long_task`/`vital`) | `event.event_context.type` (`error`, `network-request`, `log`, `user-interaction`, `web-vitals`, `longtask`, `resources`, `custom-measurement`, ...) | No view event type in Coralogix. |
| `event.view.url` | `event.page_context.page_url` | |
| `event.error.message` | `event.error_context?.error_message` | |
| `event.resource.url` | `event.network_request_context?.url` (fetch/xhr) / `event.resource_context?.name` (static) | |
| `event.action.target.name` | `event.interaction_context?.target_element_inner_text` | |
| `event.context.<k>` | `event.labels.<k>` | |
| `event.usr.id` / `.email` | `event.session_context.user_id` / `.user_email` | |
| beforeSend dropping errors by message | `ignoreErrors: [/regex/, 'substring']` | Matched against `error_context.error_message`; string = substring match, RegExp = `test` (from source). |
| beforeSend dropping resources/requests by URL | `ignoreUrls: [/regex/]` | Drops fetch/XHR and static-resource events for matching URLs; matched fetch/XHR are not instrumented (no trace headers). Use RegExp only: string matching differs per event type (from source). |
| `excludedActivityUrls` | none | DD only excludes URLs from page-activity computation and still records them. Do not map to `ignoreUrls`. |

Pattern:
```ts
// DD
beforeSend: (e) => { if (e.type === 'error' && e.error.message.includes('ResizeObserver')) return false; e.view.url = redact(e.view.url); return true }
// Coralogix
ignoreErrors: [/ResizeObserver/],
beforeSend: (e) => { e.page_context.page_url = redact(e.page_context.page_url); return e },
```

## 7. Session Replay and privacy

| Datadog | Coralogix | Notes |
|---|---|---|
| (replay enabled via `sessionReplaySampleRate > 0`) | `sessionRecordingConfig: { enable: true, autoStartSessionRecording: true, sessionRecordingSampleRate: N }` | `enable` and `autoStartSessionRecording` are required fields. |
| `startSessionReplayRecording()` | `startSessionRecording()` | Only works if the session was sampled for recording. |
| `startSessionReplayRecording({ force: true })` | none | Cannot override recording sampling. |
| `stopSessionReplayRecording()` | `stopSessionRecording()` | |
| `workerUrl` | `sessionRecordingConfig.workerUrl` | Host Coralogix `worker.min.js` (from the same CDN version), not the DD worker. |
| `compressIntakeRequests` | none | |
| `sessionReplayCanvasRecording: { enable: true }` | `sessionRecordingConfig.recordCanvas: true` | rrweb option passed through; no `quality`. |
| `defaultPrivacyLevel` | `sessionRecordingConfig` mask options + top-level `maskInputTypes` / `maskClass` | Two separate surfaces in Coralogix: recording (rrweb: `maskAllInputs`, `maskInputOptions`, `maskTextClass`/`maskTextSelector`, `blockClass`/`blockSelector`, `ignoreClass`) and click text in interaction events (`maskInputTypes`, default `['password']`; `maskClass`, default `cx-mask`). Configure both. |

DD defaults to `mask-user-input` when `defaultPrivacyLevel` is unset. DD always masks `password`/`email`/`tel`/`hidden` inputs and `autocomplete="cc-*"`/`"*-password"` inputs at every level. Coralogix recording defaults mask only password inputs and no text. Configs below keep privacy at least as strict:

```ts
const ALL_INPUT_TYPES = ['text','password','email','tel','number','search','url','date','datetime-local','month','week','time','color','range','hidden'];
const DD_ALWAYS = 'input[type="hidden"], input[autocomplete^="cc-"], input[autocomplete$="-password"]';

// DD 'allow'
sessionRecordingConfig: { enable: true, autoStartSessionRecording: true, sessionRecordingSampleRate: N,
  maskInputOptions: { password: true, email: true, tel: true }, blockSelector: DD_ALWAYS },
maskInputTypes: ['password', 'email', 'tel', 'hidden'],

// DD 'mask-user-input' (DD default)
sessionRecordingConfig: { enable: true, autoStartSessionRecording: true, sessionRecordingSampleRate: N,
  maskAllInputs: true, blockSelector: DD_ALWAYS },
maskInputTypes: ALL_INPUT_TYPES,

// DD 'mask' and 'mask-unless-allowlisted'
sessionRecordingConfig: { enable: true, autoStartSessionRecording: true, sessionRecordingSampleRate: N,
  maskAllInputs: true, maskTextSelector: '*', blockSelector: DD_ALWAYS },
maskInputTypes: ALL_INPUT_TYPES,
maskClass: /^/,   // matches every class attribute incl. empty: masks all click text (from source)
```

Gaps to report under `mask`: Coralogix masks text nodes and input values only; it does not mask attributes (`title`, `alt`, `placeholder`, `aria-label`, `href`, `data-*`) or image `src`, which DD `mask` does. If those carry sensitive data, add the elements to `blockSelector` (e.g. `img`). UNVERIFIED: whether `maskClass` also masks `aria-label`/`title`-derived click names (`resolved_name`). Textarea/select values in click text are not covered by `maskInputTypes` (inputs only); redact `interaction_context.target_element_inner_text` in `beforeSend` if needed. `mask-unless-allowlisted` has no allowlist equivalent.

Markup. `blockSelector`, `maskTextSelector` and `ignoreSelector` are CSS selectors (single string, comma-separate), so DD privacy markup can stay unchanged for recording. Click-text masking (`maskClass`) reads class names only.

| DD markup | Coralogix | Notes |
|---|---|---|
| `data-dd-privacy="hidden"`, `.dd-privacy-hidden` | add `[data-dd-privacy="hidden"], .dd-privacy-hidden` to `blockSelector` (or class `rr-block`) | Replaced by a same-size placeholder in replay, like DD. |
| `data-dd-privacy="mask"`, `.dd-privacy-mask` | add `[data-dd-privacy="mask"], .dd-privacy-mask` to `maskTextSelector` (or class `rr-mask`) | Masks text of the element and descendants. Does not mask input values inside it: keep `maskAllInputs: true` or block. |
| click-text masking for the two rows above | `maskClass` RegExp (see code below) | Covers the class form. For the attribute form add class `cx-mask` to those elements (or switch them to the class form). |
| `data-dd-privacy="mask-user-input"`, `.dd-privacy-mask-user-input` | `maskAllInputs: true` (global) | No per-element input-masking class; `rr-ignore` only suppresses input events. |
| `data-dd-privacy="allow"`, `.dd-privacy-allow` (unmask under `mask`) | none | No per-element unmask; with `maskTextSelector: '*'` everything stays masked (stricter). |
| Default class names | `rr-block` (block), `rr-mask` (text), `rr-ignore` (input events), `cx-mask` (click text) | Configurable via `blockClass`, `maskTextClass`, `ignoreClass`, `maskClass`. |

```ts
// keep DD markup; adapt to the chosen privacy level above
sessionRecordingConfig: { /* ... */
  blockSelector: `${DD_ALWAYS}, [data-dd-privacy="hidden"], .dd-privacy-hidden`,
  maskTextSelector: '[data-dd-privacy="mask"], .dd-privacy-mask',   // use '*' under DD 'mask'
},
maskClass: /(^|\s)(cx-mask|dd-privacy-mask|dd-privacy-hidden)(\s|$)/,  // under DD 'mask' keep /^/
```

## 8. Tracing

| Datadog | Coralogix | Notes |
|---|---|---|
| `allowedTracingUrls: [...]` | `traceParentInHeader: { enabled: true, options: { propagateTraceHeaderCorsUrls: [...], allowedTracingUrls: [...] } }` | Coralogix injects into a URL when (same-origin OR matches `propagateTraceHeaderCorsUrls`) AND (`allowedTracingUrls` unset OR matches) (from source). Without `allowedTracingUrls` every same-origin request gets headers; DD injects only into listed URLs. To keep DD scope, put the same RegExp list in both options. |
| string entry `'https://api.x.com'` (DD: prefix match) | `/^https:\/\/api\.x\.com/` | Convert every string to an anchored, escaped RegExp. Coralogix string matching is not prefix-based (from source). |
| RegExp entry | same RegExp | |
| function entry `(url) => boolean` | none | Coralogix accepts `string \| RegExp` only. Rewrite as RegExp; if impossible, flag to the user. |
| `propagatorTypes: ['tracecontext']` (DD default is `['tracecontext','datadog']`) | default (no B3/X-Ray options) | Sends W3C `traceparent` (+ W3C baggage propagator) (from source). |
| `propagatorTypes: ['datadog']` (`x-datadog-trace-id`, `x-datadog-parent-id`, `x-datadog-origin`, `x-datadog-sampling-priority`) | none built in | `propagateCustomTraceHeader` accepts any OpenTelemetry `TextMapPropagator` (README has a 64-bit decimal ID example). Prefer switching the backend to W3C. UNVERIFIED: whether the user's backend tracers already extract `traceparent`; check their config before removing `x-datadog-*`. |
| `propagatorTypes: ['b3']` / `['b3multi']` | `propagateB3TraceHeader: { singleHeader: true }` / `{ multiHeader: true }` | When B3 or X-Ray is enabled, Coralogix stops sending `traceparent` (from source). To send both, also pass `propagateCustomTraceHeader: new W3CTraceContextPropagator()` from `@opentelemetry/core`. |
| (none) | `propagateAwsXrayTraceHeader: true` | Coralogix-only. |
| per-URL `propagatorTypes` | none | Propagators are global in Coralogix. |
| `propagateTraceBaggage` (user/account ids in baggage) | none | |
| `traceSampleRate` | none | |
| Backend CORS | unchanged requirement | Cross-origin APIs must allow the new header names in `Access-Control-Allow-Headers` (`traceparent`, `tracestate`, `baggage`, or `b3`/`X-B3-*`, `X-Amzn-Trace-Id`); `x-datadog-*` can be removed once DD is gone. |
| `trackResourceHeaders` | Approximation: `networkExtraConfig: [{ url, reqHeaders, resHeaders, collectReqPayload, collectResPayload }]` | DD drops sensitive header names automatically; Coralogix does not. Never list `Authorization`, `Cookie`, tokens or keys. |
| `allowedGraphQlUrls` | none | |
| `betaTrackWebSockets` | none | |

## 9. Frameworks

| Datadog | Coralogix | Notes |
|---|---|---|
| React: `plugins: [reactPlugin({ router: true })]` | remove | Views come from URLs (§2). |
| `createBrowserRouter` / `createHashRouter` / `createMemoryRouter` / `useRoutes` / `Routes` from `@datadog/browser-rum-react/react-router-v6\|v7\|v8` | same names from `react-router` / `react-router-dom` | Import swap only. |
| `createRouter` from `@datadog/browser-rum-react/tanstack-router` | `createRouter` from `@tanstack/react-router` | |
| `<ErrorBoundary fallback={Fallback}>` (fallback gets `{ error, resetError }`) | own boundary (class with `componentDidCatch(error, info)` → `CoralogixRum.captureError(error, { componentStack: info.componentStack })`) or an existing library's `onError` | Keep the fallback UI. UNVERIFIED: whether React's own `console.error` of caught errors also produces a Coralogix error (possible duplicates). |
| `addReactError(error, info)` | `CoralogixRum.captureError(error, { componentStack: info.componentStack })` | |
| `UNSTABLE_ReactComponentTracker` | none | Approximation: `startTimeMeasure`/`endTimeMeasure`. |
| Angular: `angularPlugin({ router: true })`, `provideDatadogRouter()` | remove | |
| `provideDatadogErrorHandler()` / `addAngularError(error)` | custom `ErrorHandler` whose `handleError(e)` calls `CoralogixRum.captureError(...)` and then `console.error(e)`; provide it as `{ provide: ErrorHandler, useClass: ... }` | `runOutsideAngularZone` (default true) keeps SDK timers out of Zone.js. |
| Vue: `vuePlugin({ router: true })`, `createRouter` from `@datadog/browser-rum-vue/vue-router` | remove plugin; `createRouter` from `vue-router` | |
| `app.config.errorHandler = addVueError` | `app.config.errorHandler = (err, _vm, info) => CoralogixRum.captureError(err instanceof Error ? err : new Error(String(err)), { info })` | |
| Next.js: `nextjsPlugin()`, `onRouterTransitionStart`, `<DatadogAppRouter />`, `<DatadogPagesRouter />` | remove | Init `CoralogixRum` in client-only code (e.g. the same `instrumentation-client` file, or a `'use client'` component); never on the server. |
| `addNextjsError(error)` in `error.tsx` / `global-error.tsx` | `CoralogixRum.captureError(error)` in the same `useEffect` | |
| Next.js Pages `ErrorBoundary` | as React `ErrorBoundary` above | |
| DD source-map upload (`datadog-ci sourcemaps upload`) | `@coralogix/rum-cli` or `@coralogix/webpack-plugin` / `vite-plugin` / `esbuild-plugin` | `application`/`version` must match init. See `cx-rum-setup`. |

## 10. Other

| Datadog | Coralogix | Notes |
|---|---|---|
| `plugins: [...]` (generic) | none | |
| `trackFeatureFlagsForEvents` | none | |
| `addFeatureFlagEvaluation` | labels (approximation, §5) | |
| `sessionPersistence` | none (`keepSessionAfterReload` for MPAs) | |
| `profilingSampleRate` | none | |
| `workerUrl` | `sessionRecordingConfig.workerUrl` | |
| `compressIntakeRequests` | none | |
| (none) | `stackTraceLimit`, `sdkFetchPriority`, `otelConfig`, `workerSupport`, `supportMfe`, `tracesExporter` | Coralogix-only; do not add unless asked. |

## No Coralogix equivalent (remove and report)

- `proxy` as a function; `trackingConsent` / `setTrackingConsent` (gate `init` instead); `allowedTrackingOrigins`
- `trackViewsManually`, `startView`, `setViewName`, `setViewContext*`, `getViewContext` (URL-derived views only)
- `sessionPersistence`, `trackSessionAcrossSubdomains`, `useSecureSessionCookie`, `usePartitionedCrossSiteSessionCookie`, `storeContextsAcrossPages`, `trackAnonymousUser` opt-out
- `silentMultipleInit`, `enableExperimentalFeatures`, `telemetry*SampleRate`, `allowUntrustedEvents`, `subdomain`, `remoteConfiguration*`, `replica`, `betaEnableViewUpdates`, `getInitConfiguration`, `getInternalContext`
- `traceSampleRate`, `traceContextInjection`, `propagateTraceBaggage`, function matchers in `allowedTracingUrls`, per-URL `propagatorTypes`, built-in `datadog` propagator (custom propagator possible)
- `excludedActivityUrls`, `allowedGraphQlUrls`, `betaTrackWebSockets`, `trackFeatureFlagsForEvents`, `plugins`
- `profilingSampleRate`, `compressIntakeRequests`
- `startSessionReplayRecording({ force: true })`, `getSessionReplayLink`
- Replay: per-element `allow` override, per-element `mask-user-input`, attribute/image masking under `mask`, `mask-unless-allowlisted` allowlist, canvas `quality`
- Exact custom actions (`addAction`, `startAction`/`stopAction`), `startResource`/`stopResource`, operations, `addDurationVital` start time/context (approximations exist)
- Logs: named loggers, logger context/tags, `setLevel`, `setHandler`, `forwardConsoleLogs` for non-error levels, `forwardReports`, beforeSend `context` argument
- `UNSTABLE_ReactComponentTracker`; router plugins of `-rum-react`/`-angular`/`-vue`/`-nextjs` (not needed)
