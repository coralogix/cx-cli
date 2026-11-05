# Sentry browser SDK → Coralogix browser RUM SDK mapping

Verified against: @sentry/* 11.11.0, @coralogix/browser 4.4.0 (2026-11-05)

Sources: `@coralogix/browser` `src/types.d.ts`, `src/session/session.model.d.ts`, `README.md`, and the
shipped bundle `index.esm2.js` (behaviour noted as "bundle" below); `@sentry/core` `types/options.d.ts`,
`types/browseroptions.d.ts`, `@sentry/browser` `client.d.ts`/`index.d.ts`, `@sentry/{react,angular,vue,nextjs}`
type declarations; docs.sentry.io (options, replay privacy, Next.js manual setup).

Rules for the agent applying this file:

- Every Coralogix call below exists in `CoralogixOtelWebType` / `CoralogixBrowserSdkConfig`. Do not call anything else.
- `CoralogixRum.*` setters (`setUserContext`, `setLabels`, `captureError`, `log`, ...) are no-ops before
  `CoralogixRum.init()` (bundle: they `console.debug` and return). Init must run first.
- Coralogix rates are **percent, 0–100** (bundle: `threshold > 0 && Math.random() * 100 <= threshold`).
  Sentry rates are **fractions, 0–1**. Multiply by 100.
- Anything marked `UNVERIFIED:` must not be applied silently. Flag it to the user.
- Mobile SDKs (React Native, Flutter, iOS, Android) are out of scope.

## 1. Packages and files

Coralogix ships a single browser package, `@coralogix/browser`. It has no React, Angular, Vue or Next.js
packages (README "Supported Frameworks" lists the same package for all of them).

| Sentry | Coralogix | Notes |
|---|---|---|
| `@sentry/browser` | `@coralogix/browser` | `import { CoralogixRum } from '@coralogix/browser'` |
| `@sentry/react` | `@coralogix/browser` | ErrorBoundary/router helpers have no packaged equivalent, see §9 |
| `@sentry/angular` | `@coralogix/browser` | Custom `ErrorHandler`, see §9 |
| `@sentry/vue` | `@coralogix/browser` | `app.config.errorHandler`, see §9 |
| `@sentry/nextjs` (client part) | `@coralogix/browser` | Client only, see Next.js table |
| `@sentry/replay`, `@sentry/replay-canvas`, `@sentry/feedback` (re-exported by `@sentry/browser`) | built into `@coralogix/browser` (recording only) | Recording is lazy-loaded (`sessionRecorder.esm.js`). Canvas: rrweb `recordCanvas` option. Feedback: no equivalent |
| `@sentry/webpack-plugin`, `@sentry/vite-plugin`, `@sentry/esbuild-plugin`, `withSentryConfig` source-map upload | `@coralogix/webpack-plugin`, `@coralogix/vite-plugin`, `@coralogix/rum-cli` | Uploads must use the same `application` + `version` as `CoralogixRum.init` (rum-cli README). Source-map setup is not covered in this file |
| Loader script / CDN bundle | Coralogix CDN bundle | README § CDN. Not covered here |

### Next.js files

The Coralogix browser SDK runs only in the browser. Server and edge Sentry SDKs have no RUM equivalent.
Server-side observability goes to Coralogix APM via OpenTelemetry. **This skill does not migrate those files.**

| Sentry file / construct | Coralogix | Notes |
|---|---|---|
| `instrumentation-client.ts` (`Sentry.init(...)`) | Same file, `CoralogixRum.init(...)` | Next.js runs it in the browser before hydration |
| `export const onRouterTransitionStart = Sentry.captureRouterTransitionStart` (in `instrumentation-client.ts`) | Remove | Coralogix tracks page changes from the URL. `trackSoftNavigations: true` is experimental (README) |
| `sentry.client.config.ts` (older layout) | Move the init into `instrumentation-client.ts`, or into a `'use client'` component mounted in the root layout | Init must run once, in the browser only (guard `typeof window !== 'undefined'`) |
| `sentry.server.config.ts` | **Not migrated** | Node runtime. Use Coralogix APM / OpenTelemetry |
| `sentry.edge.config.ts` | **Not migrated** | Edge runtime. No Coralogix browser equivalent |
| `instrumentation.ts` (`register()`, `onRequestError = Sentry.captureRequestError`) | **Not migrated** | Server-side. Delete Sentry imports only when server/edge Sentry is removed as well |
| `app/global-error.tsx` / `error.tsx` with `Sentry.captureException(error)` | `CoralogixRum.captureError(error)` | See §9 |
| `pages/_error.tsx` with `captureUnderscoreErrorException` | `CoralogixRum.captureError(err)` on the client only | Server render of `_error` has no browser SDK |
| `next.config.*` `withSentryConfig(config, {...})` | Remove the wrapper | Source-map upload → Coralogix plugin or rum-cli |
| `withSentryConfig` `tunnelRoute` | `proxyUrl` + your own route handler | See §2 `tunnel` |
| `wrapApiHandlerWithSentry`, `wrapServerComponentWithSentry`, `wrapGetServerSidePropsWithSentry`, etc. | **Not migrated** | Server-side wrappers. Unwrap only if server Sentry is removed |

## 2. Init options

`application` and `version` are required (bundle `REQUIRED_CONFIG_KEYS`; init aborts without them).
`public_key` is required unless `proxyUrl` is set. One of `coralogixDomain` / `coralogixDomainUrl` is required.

| Sentry | Coralogix | Notes |
|---|---|---|
| `dsn` | `public_key` | Different credential. The DSN embeds a Sentry project key and ingest host. `public_key` is a Coralogix RUM public key, sent as `Authorization: Bearer <public_key>` (bundle). It cannot be derived from the DSN; ask the user for it. Read it from an env var the same way the DSN was read |
| (no option; region is implied by the DSN host) | `coralogixDomain` | One of `EU1`, `EU2`, `US1`, `US2`, `US3`, `AP1`, `AP2`, `AP3`: the region of the Coralogix account, unrelated to the Sentry region. Use `coralogixDomainUrl` for an explicit ingress URL (takes precedence) |
| (none) | `application` | Required. No Sentry option. Derive from the project (`package.json` `name`) or the Sentry project name. Must match the source-map upload |
| `release` | `version` | Required in Coralogix (optional in Sentry). If `release` came from `SENTRY_RELEASE` / `getSentryRelease()`, switch to your own build variable |
| `dist` | No equivalent | Fold into `version` if it matters |
| `environment` | `environment` | Sentry defaults to `"production"`. Coralogix has no documented default: set it explicitly |
| `debug` | `debug` | Same meaning (console diagnostics) |
| `enabled: false` | No option | Skip the `CoralogixRum.init()` call under the same condition |
| `tunnel` (string; v11 also `true` / options for the managed tunnel) | `proxyUrl` (+ `ignoreProxyUrlParams`) | See "Proxy" below. Semantics differ |
| `sendDefaultPii` (not in 11.11.0 types; docs: deprecated) / `dataCollection` (v11) | `collectIPData` (default `true`), `networkExtraConfig` (headers/payloads, opt-in per URL) | To keep "no IP" behaviour set `collectIPData: false`. Coralogix collects no request headers or bodies unless `networkExtraConfig` names them |
| `dataCollection.userInfo` | `user_context` / `setUserContext` | Coralogix sets user fields only from your code |
| `initialScope` | `labels`, `user_context` in init | §4 |
| `maxBreadcrumbs`, `beforeBreadcrumb` | No equivalent | §5 |
| `attachStacktrace` | No equivalent | Coralogix stack traces come from the `Error` object |
| (none) | `stackTraceLimit` | Sets `Error.stackTraceLimit` globally (bundle) |
| `maxValueLength`, `normalizeDepth`, `normalizeMaxBreadth` | No equivalent | `stringifyCustomLogData: true` stringifies custom-log `data` |
| `integrations`, `defaultIntegrations` | `instrumentations: { errors, fetch, xhr, custom, long_tasks, resources, interactions, web_vitals }` | All default on except TBT web vital. Turning off both `fetch` and `xhr` also stops trace-header propagation |
| `transport`, `transportOptions` | No equivalent | `proxyUrl`, `sdkFetchPriority`, `otelConfig` (batch sizes) are the only transport knobs |
| `skipBrowserExtensionCheck`, `spotlight`, `cdnBaseUrl` | No equivalent | Drop |
| `routeProvider` | `urlBlueprinters.pageUrlBlueprinters` | Built-in blueprinting already replaces UUIDs and all-digit segments with `{id}` |
| `enhanceFetchErrorMessages` | No equivalent | Drop |

### Proxy (`tunnel` → `proxyUrl`)

- Sentry `tunnel`: the browser POSTs Sentry envelopes to your endpoint; the endpoint forwards the body to the
  Sentry host taken from the envelope's DSN (docs: forward only the body, accept only your own DSNs).
- Coralogix `proxyUrl`: the SDK POSTs to `${proxyUrl}?cxforward=<url-encoded Coralogix ingress URL>` (bundle).
  The proxy must:
  1. read `cxforward`, and allow only `https://ingress.<region>.rum-ingress-coralogix.com/...` (or your
     `coralogixDomainUrl`) so it is not an open relay;
  2. POST the unchanged body to that URL, keeping `Content-Type`, `Content-Encoding` and `Authorization`
     (or inject `Authorization: Bearer <public_key>` server-side, in which case `public_key` can be omitted
     from the browser config: bundle accepts `proxyUrl` without `public_key`);
  3. return the upstream status.
- Paths seen in the bundle: `/browser/v1beta/logs` (events) and `/browser/alpha/sessionrecording` (recordings).
  Both go through the proxy.
- `proxyUrl` must not already contain a query string: the SDK appends `?cxforward=` literally.
- `ignoreProxyUrlParams: true` sends to `proxyUrl` with no `cxforward`; the proxy must then know the ingress URL.
- The SDK adds `proxyUrl` to `ignoreUrls` automatically, so its own traffic is not recorded.
- A Sentry tunnel cannot be reused: the payload format and upstream are different.

## 3. Sampling

| Sentry | Coralogix | Notes |
|---|---|---|
| `sampleRate` (0–1, **per error event**, default 1.0) | No per-event equivalent | Coralogix samples **whole sessions**. To keep all errors while sampling sessions use `sessionConfig.excludeFromSampling: ['errors']` (errors still sent from unsampled sessions) or `alwaysTrackSessionsWithErrors: true`. To approximate error sampling, drop in `beforeSend` with `Math.random()`; this is not equivalent |
| `tracesSampleRate` (0–1, per trace) | `sessionConfig.sessionSampleRate` (0–100, **per session**, default 100) | Coralogix has no separate trace sampling: a sampled session records all network spans. Use `tracesSampleRate × 100` only if the customer accepts that errors and logs are sampled too (combine with `excludeFromSampling` / `alwaysTrackSessionsWithErrors`) |
| `tracesSampler(ctx)` | No equivalent | No per-trace or per-route callback. Pick a fixed `sessionSampleRate` and flag it |
| (none) | `sessionSampleRate` (top level) | Also exists and is sampled **independently** of `sessionConfig.sessionSampleRate` (bundle: both must pass). Set only `sessionConfig.sessionSampleRate`, as the README does |
| `replaysSessionSampleRate` (0–1 of all sessions) | `sessionRecordingConfig.sessionRecordingSampleRate` (0–100, default 100) | Applied **after** session sampling (README). Effective recording share = `sessionSampleRate × sessionRecordingSampleRate / 100`. To reproduce Sentry rate `r` with session rate `s`: `sessionRecordingSampleRate = min(100, r × 100 × 100 / s)` |
| `replaysOnErrorSampleRate` | `sessionConfig.onlyWithErrorConfig: { enable: true, maxRecordTime }` or `alwaysTrackSessionsWithErrors: true` | No separate rate. `onlyWithErrorConfig` buffers events (`maxRumEvents` default 5000, max 20000) and recording (`maxRecordTime` default 10000 ms, max 60000) and sends them when an error occurs. Sentry buffers up to ~60 s and keeps recording until the session ends. UNVERIFIED: whether Coralogix keeps recording after the error in this mode |
| `profileSessionSampleRate` | No equivalent | No profiling |

Example (Sentry `tracesSampleRate: 0.2, replaysSessionSampleRate: 0.1, replaysOnErrorSampleRate: 1.0`):

```ts
CoralogixRum.init({
  // ...
  sessionConfig: { sessionSampleRate: 20, alwaysTrackSessionsWithErrors: true },
  sessionRecordingConfig: { enable: true, autoStartSessionRecording: true, sessionRecordingSampleRate: 50 }, // 20% × 50% = 10%
});
```

## 4. User, tags, context

| Sentry | Coralogix | Notes |
|---|---|---|
| `setUser({ id, email, username, ...rest })` | `setUserContext({ user_id: String(id), user_name: username ?? String(id), user_email: email, user_metadata: rest })` | `user_id` and `user_name` are **required strings** in `UserContextConfig`. Optional: `account_id`, `account_name`. The call **replaces** the whole user context (bundle) |
| `setUser({ ip_address })`, `geo` | No equivalent | IP/geo collected server-side when `collectIPData` is true |
| `setUser(null)` (logout) | `setUserContext({ user_id: '', user_name: '' })` + optionally `createNewSession()` | Empty strings are the SDK defaults (bundle `OPTIONS_DEFAULTS`). No clear API |
| `initialScope: { user }` | `user_context` in init | Same shape as `setUserContext` |
| `setTag(k, v)` | `setLabels({ ...CoralogixRum.getLabels(), [k]: v })` | `setLabels` **replaces** all global labels (bundle `setCustomLabels` overwrites); always spread `getLabels()` |
| `setTags(obj)` | `setLabels({ ...CoralogixRum.getLabels(), ...obj })` | Same |
| `initialScope: { tags }` | `labels` in init | Static labels |
| `setContext(name, obj)` | `setLabels({ ...getLabels(), [name]: obj })` | Labels are `Record<string, any>`, so nested objects are allowed. `setContext(name, null)`: rebuild without the key |
| `setExtra(k, v)` / `setExtras(obj)` | Labels as above, or per-call `customData` on `captureError` | No separate "extra" bucket |
| `setAttribute(s)` (v11) | Labels | Same pattern |
| `withScope(scope => { scope.setTag(...); captureException(e) })` | `captureError(e, customData, { tag: value })` | No scopes. Per-call labels are merged over global labels for that event only (bundle `{ ...global, ...mfeLabels, ...call }`) |
| `withIsolationScope`, `getCurrentScope()`, `getIsolationScope()`, `getGlobalScope()`, `Scope` | No equivalent | Rewrite to global labels or per-call labels |
| `scope.setFingerprint`, `scope.setLevel`, `scope.setTransactionName` | No equivalent | Fingerprinting is server-side |
| `addEventProcessor(fn)` | `beforeSend` | Only one `beforeSend` exists; combine processors into it. `labelProviders` can add labels from URL or event |

## 5. Capture APIs

| Sentry | Coralogix | Notes |
|---|---|---|
| `captureException(err)` | `captureError(err)` | Signature: `captureError(error: Error, customData?: unknown, labels?: CoralogixRumLabels): void`. Must be an `Error`; wrap others: `err instanceof Error ? err : new Error(String(err))` |
| `captureException(err, { tags, extra, contexts, user, level })` | `captureError(err, extra/contexts, tags)` | `customData` lands in `error_context.error_custom_data`. No per-call `user` or `level` |
| return value `eventId` / `lastEventId()` | None | `captureError` returns `void`. Remove code that displays or links event IDs |
| `captureMessage(msg)` / `(msg, level)` | `CoralogixRum.info(msg)` etc. | Produces a **log** event, not an error/issue. Level map: `fatal`→`critical`, `error`→`error`, `warning`→`warn`, `log`→`info`, `info`→`info`, `debug`→`debug` |
| `captureMessage(msg, { extra, tags })` | `CoralogixRum.info(msg, data, labels)` | `data` is required (may be `null`) when passing `labels` (README) |
| (none) | `CoralogixRum.log(CoralogixLogSeverity.X, msg, data?, labels?)` | `CoralogixLogSeverity`: `Debug`, `Verbose`, `Info`, `Warn`, `Error`, `Critical`. Shortcut methods: `debug`, `verbose`, `info`, `warn`, `error`, `critical`. The README example `CoralogixRum.warning(...)` does not exist in the types: use `warn` |
| `captureEvent(event)` | No equivalent | Use `captureError` or a log |
| `captureFeedback`, `showReportDialog` | No equivalent | Remove UI or replace with own form |
| `addBreadcrumb(...)` | No equivalent API | Coralogix records clicks (`interactions`), network (`fetch`/`xhr`), resources, page views and console errors as separate events in the session timeline, plus session recording. To keep a manual trail, `CoralogixRum.debug(message, data)` is the closest; it creates separate log events (counted, searchable) and is **not equivalent** to breadcrumbs attached to the error |
| `flush()`, `close()` | `shutdown()` | UNVERIFIED: whether `shutdown()` flushes pending events |
| `startSession`, `endSession`, `captureSession` | `createNewSession()` | Sessions rotate after 15 min idle or 1 h (README). `getSessionId()` reads the current ID |
| `setMeasurement(name, value)` | `sendCustomMeasurement(name, value)` | Number only |

## 6. Filtering

| Sentry | Coralogix | Notes |
|---|---|---|
| `beforeSend(event, hint)` | `beforeSend(event: EditableCxRumEvent) => EditableCxRumEvent \| null \| undefined` | Coralogix calls it for **every** event type (errors, network, logs, interactions, web vitals, ...). Check `event.event_context.type`. Synchronous only (no Promise). No `hint` / `originalException`. Returning a value missing `version_metadata`, `session_context`, `page_context` or `event_context` drops the event (bundle) |
| `beforeSendTransaction` | `beforeSend` filtered on `event_context.type === 'network-request'` | No transaction object |
| `beforeSendSpan` | `beforeSend` (network/custom-span events) or `tracesExporter` | `tracesExporter` **redirects** span data away from Coralogix (README) |
| `beforeBreadcrumb` | No equivalent | Drop; filter the underlying events in `beforeSend` |
| `beforeSendLog` | `beforeSend` filtered on `event_context.type === 'log'` | |
| `ignoreErrors` | `ignoreErrors` | Matched against `error_context.error_message`; string = substring match, RegExp = `test` (bundle). UNVERIFIED: Sentry also tests `"<Type>: <message>"`; patterns anchored on the type prefix need rewriting |
| `ignoreTransactions`, `ignoreSpans` | `ignoreUrls` (for network spans) or `beforeSend` | |
| `denyUrls` (matches the **script URL of the top stack frame** of an error) | No config equivalent | Coralogix `ignoreUrls` filters **network requests and resources**, not error source files (README, bundle). Translate to `beforeSend` (pattern below). UNVERIFIED: that `error_context.original_stacktrace` is populated when `beforeSend` runs |
| `allowUrls` | No config equivalent | Same `beforeSend` pattern, inverted |
| `thirdPartyErrorFilterIntegration` | No equivalent | Same `beforeSend` pattern |
| (none) | `ignoreUrls` | Drops `fetch`/`xhr` requests and resource-timing entries. **Use RegExp entries only**: for resources a string entry matches only if the pattern contains the whole URL (bundle `isPatternMatch`) |

Event-shape translation:

| Sentry `ErrorEvent` | Coralogix `EditableCxRumEvent` |
|---|---|
| `event.exception.values[0].type` | `event.error_context?.error_type` |
| `event.exception.values[0].value`, `event.message` | `event.error_context?.error_message`, `event.log_context?.message` |
| `event.exception.values[0].stacktrace.frames` | `event.error_context?.original_stacktrace` (`fileName`, `lineNumber`, `columnNumber`, `functionName`; top frame first) |
| `event.request.url` | `event.page_context.page_url` |
| `event.user` | `event.session_context` (`user_id`, `user_name`, `user_email`, `account_id`, `account_name`, `user_metadata`) |
| `event.tags`, `event.extra`, `event.contexts` | `event.labels`, `event.error_context?.error_custom_data` |
| `event.level` | `event.event_context.severity` (1–6, `CoralogixLogSeverity`) |
| `event.release`, `event.environment` | `event.version_metadata.app_version`, `event.environment` |
| `return null` | `return null` |

```ts
// Sentry
beforeSend(event, hint) {
  if (event.exception?.values?.[0]?.value?.includes('ResizeObserver')) return null;
  if (event.user?.email) event.user.email = '[redacted]';
  return event;
},
denyUrls: [/extensions\//i, /^chrome:\/\//i],

// Coralogix
beforeSend(event) {
  if (event.event_context.type === 'error') {
    if (event.error_context?.error_message?.includes('ResizeObserver')) return null;
    const top = event.error_context?.original_stacktrace?.[0]?.fileName ?? '';
    if ([/extensions\//i, /^chrome:\/\//i].some((re) => re.test(top))) return null; // was denyUrls
  }
  if (event.session_context.user_email) event.session_context.user_email = '[redacted]';
  return event;
},
```

## 7. Session Replay → Session Recording

Privacy defaults differ. Sentry masks all text, all inputs and blocks all media by default.
Coralogix (rrweb) masks only `password` inputs and elements with `rr-mask` by default. **The migration must keep
privacy at least as strict**: unless the customer explicitly relaxed Sentry's defaults, emit the strict block below.

| Sentry `replayIntegration({...})` | Coralogix `sessionRecordingConfig` | Notes |
|---|---|---|
| `replayIntegration()` present | `enable: true, autoStartSessionRecording: true` | Both are required fields in `SessionRecordingConfig` |
| `maskAllText: true` (default) | `maskTextSelector: '*'` | README's "mask all" recipe |
| `maskAllInputs: true` (default) | `maskAllInputs: true` | rrweb default is `false` |
| `blockAllMedia: true` (default) | `blockSelector: 'img,svg,video,object,picture,embed,map,audio'` | No boolean equivalent. Element list taken from Sentry docs |
| `mask: ['.x']` (+ defaults `.sentry-mask`, `[data-sentry-mask]`) | `maskTextClass` (string/RegExp, default `rr-mask`) or `maskTextSelector` (one CSS selector string) | Join selectors with `,`. Keep `.sentry-mask,[data-sentry-mask]` in the selector, or rename classes in markup |
| `block: [...]` (+ `.sentry-block`, `[data-sentry-block]`) | `blockClass` (default `rr-block`) / `blockSelector` | Blocked elements replay as same-size placeholders in both |
| `ignore: [...]` (+ `.sentry-ignore`, `[data-sentry-ignore]`) | `ignoreClass` (default `rr-ignore`) / `ignoreSelector` | Input events not recorded |
| `unmask: [...]` | No equivalent | rrweb has no unmask. With `maskTextSelector: '*'` everything stays masked; to unmask, replace `'*'` with a narrower selector and flag the change |
| `unblock: [...]` | No equivalent | Narrow `blockSelector` instead |
| `maskFn` | `maskTextFn`, `maskInputFn` | rrweb callbacks |
| `networkDetailAllowUrls`, `networkCaptureBodies`, `networkRequestHeaders` | `networkExtraConfig: [{ url, reqHeaders, resHeaders, collectReqPayload, collectResPayload }]` | Attaches to network events, not to the recording |
| `replayCanvasIntegration()` | `recordCanvas: true` | rrweb option |
| `getReplay().start()` / `.stop()` / `.flush()` | `startSessionRecording()` / `stopSessionRecording()` | With `autoStartSessionRecording: false` for manual start |
| `minReplayDuration`, `maxReplayDuration`, `stickySession` | No equivalent | `immediateFlush: true` helps short sessions |
| (none) | `recordConsoleEvents` | Records console output into the replay |
| (none) | `captureScreenshotOnDomChange` (default `true`), `screenshot()` | Screenshots use the same masking config |

```ts
sessionRecordingConfig: {
  enable: true,
  autoStartSessionRecording: true,
  sessionRecordingSampleRate: 100,            // see §3
  maskAllInputs: true,
  maskTextSelector: '*',                       // Sentry maskAllText default
  blockSelector: 'img,svg,video,object,picture,embed,map,audio,.sentry-block,[data-sentry-block]',
  ignoreSelector: '.sentry-ignore,[data-sentry-ignore]',
},
maskInputTypes: ['password', 'email', 'tel', 'text', 'number', 'search', 'url', 'date'], // click/interaction text
maskClass: 'sentry-mask',                      // or keep default 'cx-mask' and update markup
```

`maskInputTypes` (default `['password']`) and `maskClass` (default `cx-mask`) are separate from recording: they mask
the inner text that the `interactions` instrumentation captures from clicked buttons, links, labels, inputs and options.
Sentry click breadcrumbs carry selectors, so review whether click text is acceptable or set `maskClass` broadly.

## 8. Tracing

| Sentry | Coralogix | Notes |
|---|---|---|
| `browserTracingIntegration()` | `traceParentInHeader: { enabled: true }` | Network spans (`fetch`, `xhr`) are recorded for every tracked session regardless. `traceParentInHeader` only controls header propagation, and is a prerequisite for custom spans |
| `tracePropagationTargets` (default: same origin only in browsers; matched against full URL and same-origin pathname; strings partial, case-insensitive) | `traceParentInHeader.options.propagateTraceHeaderCorsUrls` for **cross-origin** URLs; `allowedTracingUrls` to **restrict** which URLs get headers | Same-origin requests get the header by default; cross-origin only when listed in `propagateTraceHeaderCorsUrls` (README). `allowedTracingUrls` filters all candidates (bundle). Use RegExp (string entries match only if the pattern contains the full URL). Relative-path regexes like `/^\/api/` must become full-URL regexes |
| Headers `sentry-trace` + `baggage` (+ `traceparent` if `propagateTraceparent: true`) | W3C `traceparent` (W3C trace-context and baggage propagators) | When `propagateB3TraceHeader` or `propagateAwsXrayTraceHeader` is set, the W3C propagators are **not** registered (bundle). `propagateCustomTraceHeader` adds a custom `TextMapPropagator` |
| Backend continues trace from `sentry-trace` | Backend must read W3C `traceparent` (OpenTelemetry default) | A backend still on a Sentry server SDK will not join Coralogix browser traces. Update its CORS `Access-Control-Allow-Headers` from `sentry-trace, baggage` to `traceparent` (plus `tracestate`, `baggage`) or requests fail preflight |
| `startSpan({ name }, cb)` / `startSpanManual` / `startInactiveSpan` | `const g = CoralogixRum.getCustomTracer()?.startGlobalSpan(name, labels); g?.startCustomSpan(child, labels).endSpan(); g?.endSpan();` | `getCustomTracer` returns `undefined` before init, without `traceParentInHeader.enabled`, or when a tracer already exists. Only **one** global span can be open; `startGlobalSpan` returns `undefined` while one is open. Child spans do not inherit labels. `g.withContext(fn)` links network/error/interaction events to the trace. `g.span` is an OpenTelemetry `Span` (`setAttribute` etc.) |
| `getCustomTracer` called twice | Store the tracer once | Second call returns `undefined` |
| `span.setAttribute`, `setHttpStatus`, `updateSpanName` | `g.span.setAttribute(...)` | OTel `Span` API on the global span only |
| `continueTrace`, `getTraceData`, `startNewTrace`, `withActiveSpan`, `suppressTracing` | No equivalent | |
| Page-load / navigation transactions, `reactRouterV*BrowserTracingIntegration`, `tanstackRouterBrowserTracingIntegration`, Vue/Angular router tracing | Automatic page views; `urlBlueprinters` for route names | No router integration needed. SPA navigations are tracked from URL changes; `trackSoftNavigations` is experimental |
| `webVitalsIntegration`, web vitals in `browserTracingIntegration` | `instrumentations.web_vitals` (default on) | TBT is opt-in: `web_vitals: { enabled: true, metrics: { tbt: true } }` |
| Long tasks / long animation frames | `instrumentations.long_tasks` (default on) | |
| `interactionsIntegration`, INP | `instrumentations.interactions` | |
| `userTimingIntegration`, `elementTimingIntegration` | `addTiming(name, duration?)`, `startTimeMeasure(name, labels)` / `endTimeMeasure(name)` | Manual |
| `fetchStreamPerformanceIntegration`, `bfcacheMetricsIntegration` | No equivalent | |

## 9. Framework integrations

Coralogix auto-captures (bundle `CoralogixErrorInstrumentation`): `window` `error` events, `unhandledrejection`,
resource-load errors on `document`, and **every `console.error(...)` call** (source `console`; the first `Error`
argument supplies the stack). Framework-caught errors that the framework logs with `console.error` are therefore
captured without extra code, but without component context and with source `console`.
UNVERIFIED: which framework versions log caught errors through `console.error` in production builds (React,
Angular's default `ErrorHandler` and Vue 3 are commonly believed to). Install the explicit handlers below to keep
component context. **If a handler calls `captureError` and then `console.error` for the same error, Coralogix
records it twice** (once `captured`, once `console`); do not re-log, or accept the duplicate.

| Sentry | Coralogix | Notes |
|---|---|---|
| `Sentry.ErrorBoundary` / `withErrorBoundary(C, opts)` | Own boundary calling `captureError` in `componentDidCatch` | `fallback`, `onError`, `onReset` must be reimplemented; `showDialog` has no equivalent; `beforeCapture(scope)` → per-call labels |
| `reactErrorHandler()` on `createRoot({ onCaughtError, onUncaughtError, onRecoverableError })` (React 19) | `(error, info) => CoralogixRum.captureError(toError(error), { componentStack: info.componentStack })` | Plain function, no wrapper needed |
| `captureReactException(error, errorInfo)` | `captureError(error, { componentStack: errorInfo.componentStack })` | |
| `Profiler`, `withProfiler`, `useProfiler` | No equivalent | Remove |
| `createReduxEnhancer` | No equivalent | Remove; optionally labels from state |
| Angular `{ provide: ErrorHandler, useValue: Sentry.createErrorHandler({ logErrors }) }` | Custom `ErrorHandler` class calling `captureError` | Sentry's handler defaults `logErrors: true` (calls `console.error`); with Coralogix that would duplicate |
| Angular `TraceService`, `TraceModule`, `TraceDirective`, `TraceClass`, `TraceMethod`, `provideAppInitializer(() => inject(TraceService))` | No equivalent | Remove. Coralogix runs its timers outside the Angular zone (`runOutsideAngularZone`, default `true`) |
| Vue `Sentry.init({ app, attachProps, attachErrorHandler, tracingOptions })` | `CoralogixRum.init({...})` + `app.config.errorHandler` | Sentry wraps any existing `errorHandler` and rethrows when none existed (bundle of `@sentry/vue`). Preserve a pre-existing handler by calling it |
| Vue `vueIntegration`, `createTracingMixins`, `createSentryPiniaPlugin` | No equivalent | Remove |
| Next.js `app/global-error.tsx` `useEffect(() => Sentry.captureException(error), [error])` | `useEffect(() => CoralogixRum.captureError(error), [error])` | `error.digest` → `captureError(error, { digest: error.digest })` |

```tsx
// React
import { Component, type ErrorInfo, type ReactNode } from 'react';
import { CoralogixRum } from '@coralogix/browser';

export class ErrorBoundary extends Component<{ fallback: ReactNode; children: ReactNode }, { hasError: boolean }> {
  state = { hasError: false };
  static getDerivedStateFromError() { return { hasError: true }; }
  componentDidCatch(error: unknown, info: ErrorInfo) {
    const err = error instanceof Error ? error : new Error(String(error));
    CoralogixRum.captureError(err, { componentStack: info.componentStack });
  }
  render() { return this.state.hasError ? this.props.fallback : this.props.children; }
}
```

```ts
// Angular
import { ErrorHandler, Injectable } from '@angular/core';
import { CoralogixRum } from '@coralogix/browser';

@Injectable()
export class CoralogixErrorHandler implements ErrorHandler {
  handleError(error: unknown): void {
    const err = error instanceof Error ? error : new Error(String(error));
    CoralogixRum.captureError(err); // no console.error here: Coralogix would record it again
  }
}
// providers: [{ provide: ErrorHandler, useClass: CoralogixErrorHandler }]
```

```ts
// Vue 3 (after CoralogixRum.init, before app.mount)
const previous = app.config.errorHandler;
app.config.errorHandler = (err, instance, info) => {
  const e = err instanceof Error ? err : new Error(String(err));
  CoralogixRum.captureError(e, { lifecycleHook: info, component: instance?.$options?.name });
  previous?.(err, instance, info);
};
```

## 10. Other integrations and APIs

| Sentry | Coralogix | Notes |
|---|---|---|
| `globalHandlersIntegration`, `browserApiErrorsIntegration` | `instrumentations.errors` (default on) | |
| `linkedErrorsIntegration` (`error.cause`) | No equivalent | UNVERIFIED: whether `cause` chains are reported |
| `dedupeIntegration` | No equivalent | |
| `httpContextIntegration` | Automatic `page_context`, user agent, device, browser | |
| `httpClientIntegration` (failed requests → error events) | Automatic `network-request` events; status ≥ 400 gets severity Error (bundle) | They stay network events, not error events. Headers/bodies via `networkExtraConfig` |
| `captureConsoleIntegration({ levels })` | `console.error` → error events automatically; other levels: no equivalent | `recordConsoleEvents` puts all console output in the replay only |
| `consoleLoggingIntegration`, `createConsolaReporter` | No equivalent | Call `CoralogixRum.info/warn/...` explicitly |
| `Sentry.logger.trace/debug/info/warn/error/fatal(msg, attrs)`, `logger.fmt` | `CoralogixRum.debug/debug/info/warn/error/critical(msg, attrs)` | `trace` → `debug` (or `verbose`). `fmt` template → plain string. Older SDKs needed `enableLogs: true`; no Coralogix flag (custom logs on by default, `instrumentations.custom`) |
| `Sentry.metrics.count/gauge/distribution(name, value, opts)` | `sendCustomMeasurement(name, value)` | Partial: single number, no attributes/units/aggregation type. For counts, send a log event and derive a metric with Coralogix Events2Metrics |
| `contextLinesIntegration` | No equivalent | Source context comes from uploaded source maps (`cx rum stacktrace`) |
| `rewriteFramesIntegration` | No equivalent | Fix paths at source-map upload time |
| `extraErrorDataIntegration` | `captureError(err, customData)` | Manual |
| `reportingObserverIntegration` | No equivalent | |
| `feedbackIntegration` / `feedbackAsyncIntegration` / `sendFeedback` / `getFeedback` | No equivalent | |
| `browserProfilingIntegration`, `uiProfiler` | No equivalent | |
| `browserSessionIntegration` (release health) | Sessions are always tracked | No crash-free-rate equivalent |
| `featureFlagsIntegration`, LaunchDarkly/OpenFeature/Unleash/Statsig/GrowthBook integrations | No equivalent | Put active flags in labels |
| `graphqlClientIntegration`, `supabaseIntegration`, `zodErrorsIntegration`, `moduleMetadataIntegration` | No equivalent | |
| `webWorkerIntegration`, `registerWebWorker` | `workerSupport: true` | README § Web Worker Support |
| `spotlightBrowserIntegration`, `diagnoseSdkConnectivity` | No equivalent | Use `debug: true` |
| `makeBrowserOfflineTransport`, `makeMultiplexedTransport` | No equivalent | Micro-frontends: `supportMfe: true` + Coralogix build plugin |
| `lazyLoadIntegration`, `addIntegration` | No equivalent | Configure everything at init |
| Cron monitors (`withMonitor`, `captureCheckIn`) | No equivalent (server-side feature) | Not migrated |
| `setConversationId`, AI integrations | No equivalent in the browser SDK | |

## No Coralogix equivalent (summary)

- Per-error-event sampling (`sampleRate`), per-trace sampling (`tracesSampleRate` independent of sessions), `tracesSampler`
- Separate on-error replay rate (`replaysOnErrorSampleRate`); `replayIntegration` `unmask` / `unblock`, `blockAllMedia` boolean, `minReplayDuration` / `maxReplayDuration`
- Breadcrumbs: `addBreadcrumb`, `beforeBreadcrumb`, `maxBreadcrumbs`
- Scopes: `withScope`, `withIsolationScope`, scope fingerprint/level/transaction name
- `denyUrls` / `allowUrls` as config (only via `beforeSend`), `thirdPartyErrorFilterIntegration`
- Async `beforeSend`, `hint.originalException`
- Event IDs (`captureException` return value, `lastEventId`), `captureEvent`
- `dist`, `enabled`, `transport`, `attachStacktrace`, `normalizeDepth`, `maxValueLength`
- User feedback widget and report dialog
- Profiling, cron monitors, release-health crash-free rates
- Sentry metrics with attributes (only `sendCustomMeasurement`)
- `captureConsoleIntegration` for levels other than `error`
- `linkedErrorsIntegration`, `dedupeIntegration`, `contextLinesIntegration`, `rewriteFramesIntegration`, `reportingObserverIntegration`
- Framework packages: React `ErrorBoundary`/`Profiler`/Redux enhancer, Angular `TraceService` family, Vue tracing mixins/Pinia plugin (replace error handling manually, drop the rest)
- Router tracing integrations (not needed: page tracking is URL-based)
- All server/edge Next.js instrumentation (`sentry.server.config.ts`, `sentry.edge.config.ts`, `instrumentation.ts`, `captureRequestError`, server wrappers): out of scope, use Coralogix APM / OpenTelemetry
