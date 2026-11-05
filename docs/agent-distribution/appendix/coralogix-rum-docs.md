# Coralogix RUM: verified facts for the install skill

Researched 2026-11-05. Docs were read with `cx docs fetch <suffix>`. A suffix `X.md` maps to `https://coralogix.com/docs/X/`. I checked this mapping once: `user-guides/rum/cli/rum-cli.md` maps to https://coralogix.com/docs/user-guides/rum/cli/rum-cli/ (H1 "Official Coralogix RUM CLI"). Package versions come from the public npm registry (`--registry=https://registry.npmjs.org`), Maven Central, CocoaPods trunk, pub.dev and GitHub, all as of today.

Abbreviations used below:
- `D:` is a path relative to https://coralogix.com/docs/
- `RN` is React Native

---

## 1. SDK packages and current versions

| Platform | Package / coordinates | Latest | Source |
|---|---|---|---|
| Browser (npm) | `@coralogix/browser` | **4.4.0** (2026-11-02) | `npm view @coralogix/browser version`; D:user-guides/rum/sdk-installation/javascript/release-notes.md |
| Browser (CDN) | `https://cdn.rum-ingress-coralogix.com/coralogix/browser/[version]/coralogix-browser-sdk.js`, with an ES5 build at `.../coralogix-browser-sdk.es5.js` | same as npm | D:user-guides/rum/sdk-installation/javascript/cdn-browser.md |
| React Native (current) | `@coralogix/react-native-plugin` | **0.15.0** | npm; D:user-guides/rum/sdk-installation/react-native/react-native-plugin.md |
| React Native (legacy) | `@coralogix/react-native-sdk` | 1.0.26 (replaced by the plugin) | npm; D:user-guides/rum/sdk-installation/react-native/react-native-sdk.md |
| Flutter (mobile) | `cx_flutter_plugin` (`flutter pub add cx_flutter_plugin`) | **0.20.0** | pub.dev/packages/cx_flutter_plugin; D:user-guides/rum/sdk-installation/flutter/mobile.md |
| Flutter (web) | Uses the browser CDN SDK through `dart:js` | — | D:user-guides/rum/sdk-installation/flutter/web.md |
| iOS / tvOS (SPM) | `git@github.com:coralogix/cx-ios-sdk`, rule "Up to Next Major Version"; products `Coralogix`, `SessionReplay` | **2.22.0** | D:user-guides/rum/sdk-installation/apple/ios.md; `gh api repos/coralogix/cx-ios-sdk/releases/latest`; Package.swift |
| iOS (CocoaPods) | `pod 'Coralogix'`, plus `pod 'SessionReplay'` if you use replay | **2.22.0** (trunk, 2026-10-29) | same doc; trunk.cocoapods.org/api/v1/pods/Coralogix |
| Android | `implementation "com.coralogix:android-sdk:2.25.0"` | **2.25.0** | D:user-guides/rum/sdk-installation/android.md; repo1.maven.org maven-metadata.xml |
| Android Gradle plugin (OkHttp auto-instrumentation only) | `classpath "com.coralogix.gradle.plugin:gradle-plugin:0.0.2"`, then `apply plugin: "com.coralogix.gradle.plugin"` | 0.0.2 as documented | react-native-plugin.md |
| Source-map CLI | `@coralogix/rum-cli`, binary `coralogix-rum-cli` | **1.1.49** | npm; D:user-guides/rum/cli/rum-cli.md |
| Bundler plugins (micro-frontend metadata only) | `@coralogix/vite-plugin`, `@coralogix/webpack-plugin`, `@coralogix/esbuild-plugin` | 0.12.0 each | npm; D:user-guides/rum/sdk-installation/javascript/bundler-plugins/{vite,webpack,esbuild}.md |

Platform requirements:
- **iOS:** iOS 13 or later, and tvOS 14 or later when installed with SPM (ios.md).
- **Android:** `minSdk = 24`. API 24 and 25 need core library desugaring (android.md).
- **Flutter:** 3.27.0 or later. Release builds on Android also need the `INTERNET` permission in the **main** manifest (flutter/mobile.md).

### Framework-specific notes

| Framework | What the docs say | Source |
|---|---|---|
| Next.js | Has a dedicated page. Create `app/coralogix.tsx` with `'use client'` that calls `CoralogixRum.init(...)` and returns `null` (SDK 2.1.0 and later). Render `<CoralogixRumInit/>` inside `<body>` in the root layout. For SDK versions below 2.1.0, use a dynamic `import()` inside `useEffect`. | D:user-guides/rum/sdk-installation/javascript/nextjs.md |
| SvelteKit | Has a dedicated page. Call `CoralogixRum.init` inside `onMount` in the layout file. | D:.../javascript/sveltekit.md |
| Angular | No install page. The init option `runOutsideAngularZone` defaults to `true`, so SDK timers run outside Zone.js. The changelog mentions a "zoneless CDN build for Angular consumers" and that SDK web APIs run outside zone.js by default. | npm README of `@coralogix/browser` ("Angular Zone.js Configuration"); browser release-notes.md |
| React / Vue | No dedicated page. The README lists React, Angular, Vue and NextJS as supported. **No React error-boundary integration is documented.** The documented manual API is `CoralogixRum.captureError(error, customData?, labels?)`. | npm README ("Supported Frameworks", "Manually Capture Errors") |

---

## 2. Browser `init` options (exact names)

### Where these names come from

- **Type definitions:** `CoralogixBrowserSdkConfig` in `@coralogix/browser@4.4.0/src/types.d.ts`, read through unpkg.
- **Basic parameters and setup:** D:.../javascript/npm-browser.md.
- **Configuration options:** D:.../browser-sdk/configuration/options.md.
- **Sampling:** D:.../browser-sdk/configuration/sampling.md.
- **Session recording:** D:.../browser-sdk/features/session-recording.md.
- **Session replay settings and defaults:** D:user-guides/rum/product-features/session-replay/web.md.

### Basic options

| Option | Type / default | Notes |
|---|---|---|
| `public_key` | string | The RUM key, which has the SendRUMData preset (see section 5). The 4.4.0 types mark it optional (`public_key?`), but every doc example sets it. |
| `application` | string, **required** | Must exactly match `-a` in the rum-cli upload. |
| `version` | string, **required** in the types | Must exactly match `-v` in the rum-cli upload. |
| `environment` | string, optional | Used in the Next.js, SvelteKit and README examples. It is missing from the interface printed on npm-browser.md but present in the 4.4.0 types. |
| `coralogixDomain` | `'EU1' \| 'EU2' \| 'US1' \| 'US2' \| 'US3' \| 'AP1' \| 'AP2' \| 'AP3'` | The type is `keyof CoralogixDomainsApiUrlMap`, which also has an internal `STAGING` key (src/constants.d.ts). Since a recent release, init aborts if the value is not a recognized region (release notes). GOV1 is **not** a key. |
| `coralogixDomainUrl` | string | Explicit ingress URL, e.g. `https://ingress.<region>.rum-ingress-coralogix.com`. You need either this or `coralogixDomain`; this one wins if both are set (README). |
| `labels` | object | Added to every event. |
| `user_context` | `{user_id, user_name, user_email, user_metadata?}` | Can also be set later. |

### Network and tracing options

| Option | Type / default | Notes |
|---|---|---|
| `traceParentInHeader` | `{ enabled: boolean, options?: { propagateTraceHeaderCorsUrls?: RegExp[], allowedTracingUrls?: RegExp[], propagateB3TraceHeader?, propagateAwsXrayTraceHeader?, propagateCustomTraceHeader? } }` | Off unless set. Needs the fetch or xhr instrumentation enabled. Cross-origin backends must be listed in `propagateTraceHeaderCorsUrls`. |
| `ignoreUrls`, `ignoreErrors` | `Array<string\|RegExp>` | — |
| `instrumentations` | `{ xhr, fetch, web_vitals, interactions, custom, errors, long_tasks, resources }` | All default to true. The TBT metric is off by default (logs-and-errors.md). |
| `networkExtraConfig` | array | Captures selected request/response headers and payloads. |

### Error and event handling options

| Option | Type / default | Notes |
|---|---|---|
| `stackTraceLimit` | number | The browser default is 10 frames. |
| `beforeSend` | `(event) => event \| null` | — |
| `stringifyCustomLogData` | boolean | — |
| `supportMfe` | boolean | Needs one of the bundler plugins. |

### Masking options for interaction events (not session replay)

| Option | Type / default | Notes |
|---|---|---|
| `maskInputTypes` | default `['password']` | A breaking change narrowed this from `['password','email','tel']` (release notes). |
| `maskClass` | default `'cx-mask'` | — |

The `data-cx-action-name` attribute is never masked. `actionNameAttribute` is available from 4.4.0.

### Sampling

**`sessionConfig`** contains:
- `sessionSampleRate`: a percentage, default 100.
- `alwaysTrackSessionsWithErrors`
- `excludeFromSampling`
- `keepSessionAfterReload`: use it for multi-page (non-SPA) apps.
- `onlyWithErrorConfig`: `{enable, maxRumEvents (default 5000), maxRecordTime (default 10000 ms), instrumentationsToSend}`

The 4.4.0 types also list a top-level `sessionSampleRate?`, but the sampling doc uses `sessionConfig.sessionSampleRate`.

### Session replay is **opt-in**

To turn it on, set `sessionRecordingConfig: { enable: true, autoStartSessionRecording: true, ... }`. The docs table marks both `enable` and `autoStartSessionRecording` as "required", and the example comment on `enable` says "Must declare."

Main keys and defaults:

| Key | Default |
|---|---|
| `sessionRecordingSampleRate` | 100, applied after `sessionSampleRate` |
| `recordConsoleEvents` | false |
| `immediateFlush` | false |
| `captureScreenshotOnDomChange` | true (from 3.24.0) |
| `recordCanvas` | false |
| `maskAllInputs` | false |
| `maskInputOptions` | `{ password: true }` |
| `maskTextClass` | `'rr-mask'` |
| `maskTextSelector` | null |
| `blockClass` | `'rr-block'` |
| `ignoreClass` | `'rr-ignore'` |
| `workerUrl` | inline worker by default |

To mask everything, use `maskAllInputs: true, maskTextSelector: '*'`.

Manual control: `CoralogixRum.startSessionRecording()` and `CoralogixRum.stopSessionRecording()`. `CoralogixRum.screenshot()` requires recording to be enabled.

**CSP needs for replay:**
- `script-src https://cdnjs.cloudflare.com/ajax/libs/pako/`
- `connect-src https://ingress.<region>.rum-ingress-coralogix.com`

The replay doc lists only eu1/eu2/us1/us2/ap1/ap2 in that host. The Android doc lists ingress hosts for all eight regions, using the pattern `https://ingress.<eu1|eu2|us1|us2|us3|ap1|ap2|ap3>.rum-ingress-coralogix.com` (D:user-guides/rum/sdk-installation/android/configuration/initialization.md).

### Session lifecycle

A session rotates after 15 minutes of inactivity or after 1 hour. `CoralogixRum.createNewSession()` is available from 3.19.0 (options.md).

---

## 3. Source-map upload (D:user-guides/rum/cli/rum-cli.md)

### Install and commands

- **Install:** `npm install -g @coralogix/rum-cli`. The binary is `coralogix-rum-cli`.
- **Web:**
  ```
  coralogix-rum-cli upload-source-maps -k <privateKey> -a <application> -v <version> -f <folderPath> -e <env> [-c <commitHash> -n <repoName> -o <orgName>]
  ```
  Only files ending in `.js.map` are picked up.
- **Other platforms:** `upload-react-native-source-maps`, `upload-dsym`, `upload-proguard`, `upload-dart-symbols`.

### Flags

| Flag | Meaning |
|---|---|
| `-k/--private-key` | The source-mapping API key |
| `-a/--application` | Application name |
| `-v/--version` | Application version |
| `-f/--folder-path` | Folder (or file) holding the artifacts |
| `-e/--env` | **The Coralogix region**: EU1, EU2, US1, US2, US3, AP1, AP2, AP3. It is *not* the app environment. |
| `-c`, `-n`, `-o` | Optional GitHub commit hash, repo name and org name |
| `--override` | Replace an existing release |
| `--merge` | Add to an existing release. Only for source-maps, RN and dart-symbols. |

### How uploads behave

- **Releases:** each app+version pair is a release. With no flag, the upload **fails if the app+version already exists**.
- **Matching:** `-a` and `-v` **must exactly match** the SDK's `application` and `version`, or the frames won't resolve.
- **Nothing is automatic:** the docs say "Nothing uploads automatically on any platform." This CLI is the **only** upload path. The Gradle plugin and the bundler plugins do not send symbols.
- **Size limits** (gzipped payload): 200 MB for source maps, RN maps and ProGuard; 400 MB for dSYM and Dart symbols.
- **Checking it worked:** a successful exit does not prove symbolication. Open Error Tracking, pick an error from that version, open the **Stack Trace** tab, and check that the frames show real source files and line numbers.

### API key

- The key needs **Source Mapping** permissions. The docs recommend the **SourceMapping** permission preset.
- The docs describe it as "a separate Team key with the SourceMapping preset" (npm-browser.md "What you need").
- The underlying permission is `SOURCE-MAPPING:UPLOADMAPPING`. Among system roles, only **Platform Admin** has it (D:user-guides/rum/permissions.md).

### Per-bundler and per-platform guidance

| Target | What the docs give |
|---|---|
| Vite / webpack / esbuild | **No source-map generation or upload guidance.** The official plugins (`coralogixVitePlugin({app, version, outputDir?})`, `new CoralogixWebpackPlugin({...})`) only write `cx-metadata.json` for micro-frontends used with `supportMfe: true`. The docs say explicitly that they do not upload. |
| Angular CLI, Next.js | **No per-bundler guidance.** The only statement is "Your bundler decides where the maps land, so point `-f` at whatever folder it wrote them to." Nothing covers enabling source maps, hidden maps, or keeping maps off the public server. |
| React Native (Metro) | Android: add `react-native bundle ... --sourcemap-output android/app/src/main/assets/index.android.bundle.map`. iOS: set `SOURCEMAP_FILE` in the "Bundle React Native code and images" Xcode build phase. Upload each platform in a **separate run**, because the operating system is inferred from "android" in the file name. |
| Android | Upload `app/build/outputs/mapping/<variant>/mapping.txt`. Pass the **file path, not the folder**, because the CLI takes the first `.txt` it finds. `--merge` is not supported. |
| iOS | Only `upload-dsym ... -f <folderPath>` is documented. **There is no Xcode build-phase or dSYM-location guidance.** |
| Flutter | Build with `flutter build appbundle --release --obfuscate --split-debug-info=build/symbols`, then upload one `*.symbols` file per run (the first plain, the rest with `--merge`). Don't rename the files. Requires CLI 1.1.44 or later. |

---

## 4. Account and trial

- **Trial:** the pricing page (https://coralogix.com/pricing/) says "14 days, all features, no credit card required". Its FAQ says the trial includes "a quota of 8 units".
- **Free tier:** the pricing page mentions **no free tier**.
- **Pricing units:** 1 unit = $1.50. Logs cost $0.42/GB, traces $0.16/GB and metrics $0.06/GB.
- **Signup URLs (checked live, nothing submitted):**
  - https://coralogix.com/freetrialsignup/ returns 200. Its title is "Start Free Trial - Coralogix", and the pricing page's "Start Free Trial" button links to it. The page says "No credit card required; 14-day access to all features".
  - https://signup.coralogix.com/ returns **301** to https://dashboard.eu2.coralogix.com/#/signup. That is an SPA hash route, so its content could not be rendered.
- **What the trial page's script shows (read from the page HTML):**
  - It posts to `https://api.eu2.coralogix.com/api/v1/signup/cross-region/lead`.
  - It pre-selects a region through `.../api/v1/region-routing/signup`.
  - It uses reCAPTCHA v3.
  - One of its error messages is "Please use your work email address."
- **Regions open to signup:** the page's region-ID regex is `US2|US3|EU2|AP1|AP2|AP3`. This suggests new signups are limited to those six, with no EU1 or US1. That matches the repo's `docs/agent-setup.md`. It is inferred from page JS, not stated in public docs.
- **`cx init --signup` (OAuth `prompt=create`):** nothing in the public docs contradicts the repo's description. The trial page's script supports it: it reads an `authorizeUrl` query parameter ("an OAuth authorize request to resume") and locks the region picker for it. A code comment there says "An OAuth authorize request (`cx signup`) belongs to one region". **The public docs do not document `cx init --signup` at all.**

---

## 5. RUM public key

### Key type and preset

- **What it is:** a **Send-Your-Data** key with the **SendRUMData** role preset.
- **Public by design:** the docs call this key public. The preset is standalone and cannot be combined with other presets.
- **Don't use SendData:** a key with the **SendData** preset carries no RUM permissions and must stay secret.
- Sources: npm-browser.md and cdn-browser.md ("What you need"); D:user-guides/account-management/api-keys/api-keys.md.

### Creating it in the UI

Go to **Settings → Users and Teams → API Keys**, select "+ Key" for the key type, then choose from the Role Presets dropdown (api-keys.md).

The Send-Your-Data management API page gives a different path, "Data Flow → API Keys". The two docs pages disagree.

### Creating it through the API

Yes, the documented API supports it. It is the gRPC call `com.coralogixapis.aaa.apikeys.v3.ApiKeysService/CreateApiKey` with `key_permissions.presets: ["SendRUMData"]` and `owner.team_id`. The caller's key needs the `ApiKeys` preset. Sources:
- D:developer-portal/apis/data-management/send-your-data-management-api.md
- D:api-reference/v5/api-keys-service.md (REST "Create API Key")

### "RUM integration" wizard in the UI

**Not found in the public docs.** No page describes a RUM onboarding wizard or where the UI shows the RUM key. The only "RUM integration" items in the docs are API endpoints:
- `GET /integrations/integrations/v1/rum/app-versions`
- "Trigger sync of RUM integration data"

Source: D:api-reference/v5/integration-service/get-rum-integration-versions-data.md.

---

## 6. Where users see RUM data

| Screen | Path | Source |
|---|---|---|
| Error Tracking | **RUM → Error Tracking** ("hover over RUM, then select Error Tracking") | D:user-guides/rum/getting-started/real-user-monitoring.md; D:user-guides/rum/product-features/error-tracking-user-manual.md |
| Sessions | **RUM → Sessions** | D:user-guides/rum/product-features/user-sessions.md |
| Releases | Shows a source-map indicator per release. The "Released" date appears only for releases that have uploaded source maps. | D:user-guides/rum/product-features/releases.md |

Who can see it: all standard roles include `RUM:VIEW` (permissions.md). RUM shows only Frequent Search (High) and Monitoring (Medium) data. Data routed to Compliance (Low) is not visible in RUM.

---

## 7. Coralogix MCP server

- **Docs:**
  - D:user-guides/mcp-server/overview.md
  - D:user-guides/mcp-server/setup.md
  - D:user-guides/mcp-server/oauth.md
- **Endpoint:** `https://api.<region>.coralogix.com/mgmt/api/v1/mcp`, with region one of eu1, eu2, us1, us2, us3, ap1, ap2, ap3 (server.json). The docs examples use eu2.
- **Hosting:** fully remote. There is no local install.
- **Auth:** OAuth 2.1 is recommended. A personal API key sent as `Authorization: Bearer` is the alternative.
- **Versions:** v2 is the default. To stay on v1, send the header `mcp-version: v1`; v1 is deprecated.
- **Olly tools:** these need the header `x-allow-costly-use: true`.

Install commands from the docs:

| Client | Command or config |
|---|---|
| Claude Code (OAuth) | `claude mcp add --transport http coralogix-server https://api.eu2.coralogix.com/mgmt/api/v1/mcp`, then run `/mcp` → Authenticate |
| Claude Code (API key) | Add `--header "Authorization: Bearer <KEY>"` to the command above |
| Cursor | `.cursor/mcp.json` → `{"mcpServers":{"coralogix-server":{"url":"https://api.eu2.coralogix.com/mgmt/api/v1/mcp"}}}`, optionally with `headers.Authorization` |
| Codex | `codex mcp add coralogix-server --url ...` and `codex mcp login coralogix-server` |

**Open source?** No.
- https://github.com/coralogix/mcp-server (Apache-2.0) holds docs, examples and registry metadata only. Its README says: "The server implementation is maintained privately by Coralogix; this repo carries no server source code."
- It is listed in the official MCP registry as **`com.coralogix/mcp-server` 2.0.0** (registry.modelcontextprotocol.io/v0/servers?search=coralogix).
- The npm package `coralogix-mcp` (2.0.1) is **third-party**. It is published by a personal account, not by Coralogix.

---

## Inconsistencies found in the docs (worth avoiding in the skill)

1. **Wrong `application` description:** npm-browser.md's parameter table describes `application` with the `user_context` text (a copy-paste error).
2. **Flutter web uses a retired CDN URL:** flutter/web.md uses the `.../browser/latest/coralogix-browser-sdk.js` CDN URL, but cdn-browser.md says "The latest CDN version option is no longer available. Use an explicit version instead."
3. **Incomplete CSP list:** the session-replay CSP list omits us3 and ap3.
4. **Two UI paths for API keys:** "Settings → Users and Teams → API Keys" (api-keys.md) versus "Data Flow → API Keys" (Send-Your-Data management API doc).
5. **License mismatch:** rum-cli.md says MIT, while the GitHub API reports `coralogix/rum-cli` as Apache-2.0.
6. **`environment` missing from the printed interface:** npm-browser.md's interface omits `environment`, although the framework pages and the 4.4.0 types include it.

## Open questions (not confirmed)

- **Signup regions:** which regions accept new signups is inferred only from the signup-page JS regex (US2, US3, EU2, AP1, AP2, AP3). No public doc states it.
- **The `#/signup` page:** its content could not be rendered, so the fields and flow are unverified.
- **RUM wizard:** whether the UI has a "RUM integration" wizard, and where the RUM public key is displayed in it. No public doc covers it.
- **Source maps by bundler:** no official doc covers how to enable source maps per bundler (Vite `build.sourcemap`, Angular `sourceMap`, Next.js `productionBrowserSourceMaps`, webpack `devtool`), or whether to keep `.map` files off the public server. Any guidance on this would be general bundler knowledge, not Coralogix docs.
- **iOS dSYM:** no doc covers dSYM location or an Xcode build-phase script.
- **React error boundary:** no official recipe exists. Wiring `componentDidCatch` to `CoralogixRum.captureError` would be our own suggestion.
- **Trial length beyond the pricing page:** whether the 14-day/8-unit trial applies in every signup region is not stated.
