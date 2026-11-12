# Coralogix Browser RUM SDK - Init Reference

Canonical init reference for `@coralogix/browser`, shared by every skill that writes RUM init code (`cx-rum-setup`, `cx-rum-migrate`). Edit it in `skills/shared/` and run `scripts/sync-shared-references.sh`.

Verified against `@coralogix/browser` 4.4.0 (`src/types.d.ts`, README). Options change between versions: if the installed version differs, check `node_modules/@coralogix/browser/src/types.d.ts` before using an option not listed here.

## Install

```bash
npm install @coralogix/browser        # or pnpm add / yarn add - match the repo's lockfile
```

One package covers every browser framework (React, Angular, Vue, Next.js, plain JS). There are no per-framework Coralogix packages. A CDN build exists (`https://cdn.rum-ingress-coralogix.com/coralogix/browser/<version>/coralogix-browser-sdk.js`, global `window.CoralogixRum`); always pin a version, `latest` is not maintained.

## Minimal init

```ts
import { CoralogixRum } from '@coralogix/browser';

CoralogixRum.init({
  public_key: import.meta.env.VITE_CX_RUM_KEY,   // RUM key (SendRUMData preset) - public by design
  application: 'my-app',                          // required; also used for source maps (-a)
  version: import.meta.env.VITE_APP_VERSION,      // required; must equal the source-map upload version (-v)
  coralogixDomain: 'EU1',                         // EU1 EU2 US1 US2 US3 AP1 AP2 AP3 - the team's region
  environment: 'production',
});
```

Required: `application`, `version`, `public_key`, and one of `coralogixDomain` / `coralogixDomainUrl` (init aborts without a domain). `CoralogixRum.isInited` tells whether init already ran.

## Config options (`CoralogixBrowserSdkConfig`)

| Option | Type | Purpose |
|---|---|---|
| `public_key` | string | RUM API key |
| `application` | string | App name (`version_metadata.app_name`) |
| `version` | string | Release (`version_metadata.app_version`) |
| `coralogixDomain` | `'EU1'`…`'AP3'` | Region; or `coralogixDomainUrl` for a custom ingress URL |
| `environment` | string | Environment label |
| `debug` | boolean | SDK console logging |
| `user_context` | `{ user_id, user_name, user_email?, account_id?, account_name?, user_metadata? }` | Initial user; `user_id` and `user_name` are required |
| `labels` | `Record<string, any>` | Labels on every event |
| `labelProviders` | array | Labels derived from page/network URLs |
| `ignoreErrors` | `(string \| RegExp)[]` | Drop errors whose message matches |
| `ignoreUrls` | `(string \| RegExp)[]` | Drop **network-request** events for matching URLs (not error source files) |
| `stackTraceLimit` | number | Frames kept per error |
| `sessionSampleRate` | number 0–100 | % of sessions tracked (default 100) |
| `sessionConfig` | `{ sessionSampleRate?, alwaysTrackSessionsWithErrors?, onlyWithErrorConfig?, keepSessionAfterReload?, excludeFromSampling? }` | Advanced sampling |
| `sessionRecordingConfig` | `{ enable, autoStartSessionRecording, sessionRecordingSampleRate?, recordConsoleEvents?, maskAllInputs?, maskTextClass?, blockClass?, maskInputOptions?, workerUrl?, … }` | Session replay (rrweb options); recording rate applies after `sessionSampleRate` |
| `maskInputTypes` | `InputType[]` | Input types masked in **interaction** (click) events (default `['password']`) - does not affect replay |
| `maskClass` | string \| RegExp | Class masked in interaction events (default `cx-mask`) - does not affect replay; replay uses `sessionRecordingConfig.maskTextClass`/`blockClass` |
| `actionNameAttribute` | string | Custom attribute for action names (default `data-cx-action-name`) |
| `instrumentations` | `{ errors, fetch, xhr, custom, long_tasks, resources, interactions, web_vitals }` | Toggle auto-instrumentation (all on by default) |
| `traceParentInHeader` | `{ enabled, options: { propagateTraceHeaderCorsUrls?, allowedTracingUrls?, propagateB3TraceHeader?, propagateAwsXrayTraceHeader?, propagateCustomTraceHeader? } }` | W3C `traceparent` propagation to backends |
| `beforeSend` | `(event: EditableCxRumEvent) => EditableCxRumEvent \| null` | Edit or drop (return `null`) any event |
| `proxyUrl` / `ignoreProxyUrlParams` | string / boolean | Send through your own proxy (SDK adds a `cxforward` query param with the real ingress path unless ignored) |
| `urlBlueprinters` | `{ pageUrlBlueprinters?, networkUrlBlueprinters? }` | Normalize URLs (ids → `{id}`) |
| `networkExtraConfig` | `{ url, reqHeaders?, resHeaders?, collectReqPayload?, collectResPayload? }[]` | Capture headers/payloads for matching requests |
| `collectIPData` | boolean | Collect client IP data |
| `trackSoftNavigations` | boolean | SPA soft-navigation vitals (experimental) |
| `supportMfe` | boolean | Micro-frontend attribution (needs `@coralogix/vite-plugin` / `@coralogix/webpack-plugin` metadata) |
| `workerSupport` | boolean | Instrument web workers |
| `runOutsideAngularZone` | boolean | Default `true`; keep it |

## Runtime API (`CoralogixRum.*`)

| Call | Purpose |
|---|---|
| `setUserContext({ user_id, user_name, ... })` / `getUserContext()` | Set/read the user |
| `setLabels(labels)` / `getLabels()` | Replace/read global labels - to add one, `setLabels({ ...CoralogixRum.getLabels(), key: v })` |
| `setApplicationContext({ application, version })` | Change app/version after init |
| `captureError(error, customData?, labels?)` | Report a handled error |
| `log(severity, msg, data?, labels?)`, `debug`/`verbose`/`info`/`warn`/`error`/`critical(msg, data?, labels?)` | Custom log events |
| `sendCustomMeasurement(name, number)` | Custom numeric measurement |
| `addTiming(name, duration?)`, `startTimeMeasure(name, labels?)` / `endTimeMeasure(name)` | Timings |
| `getCustomTracer()` | OpenTelemetry-style tracer for custom spans |
| `startSessionRecording()` / `stopSessionRecording()` / `screenshot(desc?)` | Replay control |
| `createNewSession()`, `getSessionId()`, `shutdown()` | Session lifecycle |

## Where to init, per framework

Init **once, client-side, as early as possible**.

| Framework | Location |
|---|---|
| Vite / CRA / plain React | `src/main.tsx` / `src/index.tsx`, before `createRoot(...).render` |
| Angular | `src/main.ts`, before `bootstrapApplication` / `platformBrowserDynamic().bootstrapModule` |
| Vue | `src/main.ts`, before `app.mount` |
| Next.js (App Router) | A `'use client'` component calling `init` in `useEffect` (guarded by `!CoralogixRum.isInited`), rendered in `app/layout.tsx`. Or `instrumentation-client.ts` (Next.js 15.3+, runs only in the browser). Never in server components, `instrumentation.ts`, or server/edge config |
| Next.js (Pages Router) | `pages/_app.tsx`, in `useEffect` |

## Keys and env vars

- The RUM key is public (it ships in the bundle) but goes through env config so it can rotate: `VITE_*`, `NEXT_PUBLIC_*`, `REACT_APP_*`, Angular `environment.ts`, Vue `VITE_*`/`VUE_APP_*`.
- `version` should come from the build (package version, git SHA, or CI release var) - the same value is passed to `coralogix-rum-cli -v`.
- The **source-map** key is a different, secret key (Team Key, `SourceMapping` preset). CI secret only - never in client code or committed files.

## Behaviors that surprise migrations

- **Setters replace, not merge:** `setLabels` and `setUserContext` overwrite the whole object. Per-call `labels` on `captureError`/`log` merge over the global labels for that one event. Setters do nothing before `init()`.
- **Sampling:** set only `sessionConfig.sessionSampleRate` and leave out the top-level `sessionSampleRate`. If both are set, the two rates are sampled independently, so they multiply. Rates are 0–100. `sessionRecordingSampleRate` is applied to the sessions that are already sampled.
- **`console.error` is captured as an error.** An error handler that calls `captureError` and then `console.error`s the same error reports it twice. Do one or the other.
- **`ignoreUrls` matches network requests and resources only.** Use RegExp entries, because a string entry for a resource matches only when it contains the whole URL. To filter errors by source file, use `beforeSend`.
- **Proxy:** the SDK posts to `${proxyUrl}?cxforward=<encoded ingress URL>`, so `proxyUrl` must not already have a query string. The proxy URL is excluded from network capture automatically.
- **Tracing:** the SDK sends W3C `traceparent`. Turning on B3 or X-Ray propagation replaces the W3C header. Backend CORS must allow the header that is sent. `getCustomTracer()` returns nothing unless `traceParentInHeader.enabled` is true.
- **Log severities:** the methods are `debug`, `verbose`, `info`, `warn`, `error`, `critical`. There is no `warning`.
