# Browser SDK

Package: **`@coralogix/browser`** (install with the project's package manager, e.g. `npm install @coralogix/browser`). The same SDK covers React, Vue, Angular, Svelte, Next.js and plain JS. Current docs: https://coralogix.com/docs/user-guides/rum/sdk-installation/javascript/npm-browser/ (or `cx docs fetch user-guides/rum/sdk-installation/javascript/npm-browser.md`). If an option below is rejected by the installed SDK's types, trust the types and the docs over this file.

## Init block

Every framework uses the same call. Fill the four `<...>` values from the framework table below.

```ts
import { CoralogixRum } from '@coralogix/browser';

CoralogixRum.init({
  application: '<app-name>',          // must match the source-map upload -a
  version: <version expression>,      // must match the source-map upload -v
  environment: <environment expression>,
  public_key: <public key expression>,
  coralogixDomain: '<REGION>',        // EU1 | EU2 | US1 | US2 | US3 | AP1 | AP2 | AP3
  maskInputTypes: ['password', 'email', 'tel'],
});
```

Initialize only when the key is set, so local builds without a key don't send or throw: wrap the call in `if (<public key expression>) { ... }`.

## Per framework

| Framework | Where to init | Public key | Version | Environment |
|---|---|---|---|---|
| **Vite** (React, Vue, Svelte, Preact, vanilla) | Top of the entry module (`src/main.tsx` / `src/main.ts`), before `createRoot(...)` / `createApp(...)` / `mount` | `import.meta.env.VITE_CORALOGIX_RUM_PUBLIC_KEY` | `__APP_VERSION__` (see "Version" below) | `import.meta.env.MODE` |
| **Next.js, App Router** | New file `app/coralogix-rum.tsx` (or `src/app/...`) starting with `'use client'`; calls init at module top under `typeof window !== 'undefined'` and exports a `CoralogixRumInit` component that returns `null`; render `<CoralogixRumInit />` inside `<body>` in the root `layout.tsx`. Never init in a server component. | `process.env.NEXT_PUBLIC_CORALOGIX_RUM_PUBLIC_KEY` | `process.env.NEXT_PUBLIC_APP_VERSION` (see "Version") | `process.env.NEXT_PUBLIC_APP_ENV ?? process.env.NODE_ENV` |
| **Next.js, Pages Router** | `pages/_app.tsx`, at module top under `typeof window !== 'undefined'` | same as App Router | same | same |
| **Angular** | `src/main.ts`, before `bootstrapApplication(...)` / `platformBrowserDynamic().bootstrapModule(...)`. The SDK runs its timers outside Zone.js by default (`runOutsideAngularZone: true`). | `environment.coralogixRumPublicKey` from `src/environments/environment*.ts` (create them with `ng generate environments` if missing). The RUM key is public, so Angular's environment files are an acceptable home for it. | `APP_VERSION` global via `ng build --define` (see "Version") | `environment.production ? 'production' : 'development'` |
| *(Angular, cont.)* | If `ng build` warns that a CommonJS dependency of the SDK (e.g. `error-stack-parser`) causes optimization bailouts, add it to `allowedCommonJsDependencies` in `angular.json` — the warning is harmless. | | | |
| **SvelteKit** | `onMount` in `src/routes/+layout.svelte` | `PUBLIC_CORALOGIX_RUM_PUBLIC_KEY` from `$env/static/public` | `__APP_VERSION__` via `define` in `vite.config` | `import.meta.env.MODE` |
| **Create React App** | Top of `src/index.tsx` | `process.env.REACT_APP_CORALOGIX_RUM_PUBLIC_KEY` | `process.env.REACT_APP_VERSION` | `process.env.NODE_ENV` |
| **Vue CLI** | Top of `src/main.ts` | `process.env.VUE_APP_CORALOGIX_RUM_PUBLIC_KEY` | `process.env.VUE_APP_VERSION` | `process.env.NODE_ENV` |
| **Static HTML, no bundler** | A `<script>` tag in `<head>` loading the CDN build, then `window.CoralogixRum.init({...})` | Inline literal (it is public and there is no build step) | A literal you bump per release | A literal |

CDN URL (pin an explicit version; `latest` is no longer served): `https://cdn.rum-ingress-coralogix.com/coralogix/browser/<version>/coralogix-browser-sdk.js`. Get the current version with `npm view @coralogix/browser version`. Docs: https://coralogix.com/docs/user-guides/rum/sdk-installation/javascript/cdn-browser/

## Env files

- Add the variable with an **empty** value to `.env.example` (create it if the project has none) so teammates know it exists.
- Put the real value in the git-ignored local file the framework reads: `.env.local` for Vite, Next.js, CRA and Vue CLI. Check `.gitignore` covers it (`git check-ignore .env.local`); add it if not.
- Remind the user to set the same variable in their hosting/CI build environment, since client env vars are inlined at **build** time.

## Version

`version` identifies the release and must be byte-for-byte the same string in the SDK init and in the source-map upload (`-v`). Use one env var, **`APP_VERSION`**, as the single source: CI sets it to the git short SHA (`git rev-parse --short HEAD`) for both the build and the upload; local builds fall back to the `package.json` version.

- **Vite / SvelteKit** — in `vite.config.*`:
  ```ts
  import pkg from './package.json' with { type: 'json' };
  export default defineConfig({
    define: { __APP_VERSION__: JSON.stringify(process.env.APP_VERSION ?? pkg.version) },
    // ...existing config
  });
  ```
  and declare it for TypeScript in `src/vite-env.d.ts` (or any `.d.ts`): `declare const __APP_VERSION__: string;`. If the config's import style rejects the JSON import attribute, read the version with `JSON.parse(readFileSync('./package.json', 'utf8')).version`.
- **Next.js** — in `next.config.*`: `env: { NEXT_PUBLIC_APP_VERSION: process.env.APP_VERSION ?? <package.json version> }`.
- **Angular** — build with `ng build --define "APP_VERSION='<sha>'"` in CI; in `main.ts` declare `declare const APP_VERSION: string | undefined;` and use `typeof APP_VERSION !== 'undefined' ? APP_VERSION : '<package.json version or "dev">'`.
- **CRA / Vue CLI** — set `REACT_APP_VERSION` / `VUE_APP_VERSION` from `APP_VERSION` in the build script.

If the project already has a release/version mechanism (a `VERSION` file, a release tag, a Sentry-style release env var), reuse it instead.

## Session Replay (only if the user opted in)

Add to the init options:

```ts
sessionRecordingConfig: {
  enable: true,
  autoStartSessionRecording: true,
  maskAllInputs: true,
  // maskTextSelector: '*',          // uncomment to mask all page text too
  // sessionRecordingSampleRate: 100, // percent of sampled sessions that record
},
```

- Elements with class `rr-block` are not recorded at all; class `rr-mask` masks text. Suggest them for anything sensitive the user names.
- If the site sends a **Content-Security-Policy**, replay needs `script-src https://cdnjs.cloudflare.com/ajax/libs/pako/` and `connect-src https://ingress.<region>.rum-ingress-coralogix.com` (region in lower case). Ordinary RUM needs the `connect-src` entry too.
- Docs: https://coralogix.com/docs/user-guides/rum/product-features/session-replay/web/

## Optional extras (only when asked)

- **Sampling:** `sessionConfig: { sessionSampleRate: <percent> }` (default 100). For high-traffic apps also consider `alwaysTrackSessionsWithErrors: true`.
- **Backend trace correlation:** `traceParentInHeader: { enabled: true, options: { propagateTraceHeaderCorsUrls: [/api\.example\.com/] } }`. Cross-origin APIs must allow the `traceparent` header in CORS.
- **User context:** `CoralogixRum.setUserContext({ user_id, user_name, user_email })` after login. Prefer an internal id.
- **React error boundaries** catch errors before the global handler sees them. If the app has one, call `CoralogixRum.captureError(error)` from its `componentDidCatch` (or `onError`).
- **Ignore noise:** `ignoreErrors: [/ResizeObserver loop/]`, `ignoreUrls: [...]`.
