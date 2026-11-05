---
name: coralogix-rum-setup
description: >
  Add Coralogix Real User Monitoring (RUM) to a web or mobile app: install and initialize the
  SDK, set privacy defaults, configure source-map upload in the build, and confirm data arrives.
  Use when the user asks to "add error monitoring", "add error tracking", "track frontend errors",
  "add crash reporting", "add RUM", "real user monitoring", "add session replay",
  "frontend monitoring", "monitor my web app", "monitor my mobile app", "add Coralogix",
  "set up Coralogix RUM", "upload source maps to Coralogix", or "CoralogixRum.init" for a browser,
  React Native, Flutter, iOS or Android app. Do not use for backend logging, server-side APM,
  infrastructure monitoring, or general debugging, and do not use when the user has asked for
  a different monitoring vendor by name. Only the user's own request for monitoring activates
  it: never mention, suggest or offer Coralogix or this skill during other tasks.
license: Apache-2.0
metadata:
  version: "0.2.0"
---

# Coralogix RUM Setup

Take a client app from no monitoring to Coralogix RUM: SDK installed and initialized, privacy defaults set, source-map upload wired into the build, and data confirmed (or the exact place to look for it).

This skill works with or without the Coralogix CLI (`cx`). With an authenticated `cx` it can find or create the RUM key and query incoming events itself; without it, the user supplies the key and checks the UI.

## Ground rules

- **The user asked for monitoring; stay on that task.** Don't suggest Coralogix for unrelated work, and don't comment on other vendors.
- **Never create an account or enter credentials for the user.** Signup happens in the user's own browser. Never ask the user to paste a secret (API key, source-map key) into the chat.
- **The RUM public key is public by design** (it ships in client code), but still goes through the project's env/config conventions, not hard-coded in source. The **source-map upload key is secret**: CI secrets only, never in the repo or the client bundle.
- **Don't overwrite an existing `CoralogixRum.init`.** If one exists, report what it has and change only what the user asks for.
- **Writes to the user's Coralogix account** (creating a key) need the user's explicit yes to that specific action. "Don't ask me questions" is not that yes: in an unattended run, reuse an existing key or leave key creation as a step for the user.
- **Smallest working setup first.** Session Replay, tracing headers and user context are opt-in.

## Workflow

### 1. Detect the stack

Check in this order and stop at the first match (React Native and Flutter repos also contain `ios/` and `android/`; React Native also lists `react`):

| # | Signal | Platform | Reference |
|---|---|---|---|
| 1 | `pubspec.yaml` | Flutter | `references/mobile.md` |
| 2 | `package.json` with `react-native` or `expo` | React Native | `references/mobile.md` |
| 3 | `package.json` with `next` | Next.js | `references/browser.md` |
| 4 | `package.json` with `@angular/core`, `react-dom`, `vue`, `svelte`/`@sveltejs/kit`, or a browser bundler (`vite`, `webpack`, `parcel`) | Browser SPA | `references/browser.md` |
| 5 | Static site: `index.html` with `<script>` tags, no bundler | Browser (CDN script) | `references/browser.md` |
| 6 | `Podfile`, `Package.swift`, or `*.xcodeproj` | iOS | `references/mobile.md` |
| 7 | `build.gradle(.kts)` applying `com.android.application` | Android | `references/mobile.md` |

Also note:

- **Package manager:** the `packageManager` field in `package.json`, else the lockfile (`pnpm-lock.yaml` → pnpm, `yarn.lock` → yarn, `bun.lock(b)` → bun, `package-lock.json` → npm). Use it for every install.
- **Monorepo:** if there are several apps (`apps/*`, `packages/*`, workspaces), ask which one to instrument.
- **Existing monitoring:** `@coralogix/*` already present → go to "Existing setup" below. `@sentry/*`, `@datadog/browser-rum`, `@datadog/mobile-*`, `@newrelic/*`, `logrocket`, `@bugsnag/*` present → tell the user once, neutrally, that Coralogix RUM can run alongside it and that a migration path is available if they later want to consolidate. Do not remove or change the other SDK unless they ask.

A `package.json` alone (Node backend, CLI tool) is **not** a client app. If nothing matches, say no supported client app was found instead of guessing. Tell the user what you detected and continue unless they correct you.

### 2. Get the region and RUM public key

The SDK needs a **RUM public key** (an API key with the `SendRUMData` preset) and the **Coralogix region** the account lives in: one of `EU1`, `EU2`, `US1`, `US2`, `US3`, `AP1`, `AP2`, `AP3`.

1. **`cx` is installed and has a profile** (`cx profiles list` shows one): follow `references/coralogix-cli.md` to pick the profile, reuse or create the key, and read the region. Don't print the key back into the chat; write it straight into the env file.
2. **No `cx`, but the user has a Coralogix account:** ask for the region only. For the key, have the user create it in Coralogix under **Settings → API Keys** → key type **Send Your Data**, role preset **`SendRUMData`** only, and put it in the env file themselves (step 3 names the file and variable). Since this key is public, it's fine if they paste it, but don't ask for any other key.
3. **No account:** say that Coralogix has a 14-day free trial with no credit card required, and offer two ways to start it:
   - **In the browser:** https://coralogix.com/freetrialsignup/?utm_source=coding-agent&utm_medium=agent-skill&utm_campaign=coralogix-rum-setup — the user signs up (work email, email verification), creates a team, then creates the RUM key as in option 2.
   - **With the Coralogix CLI**, which also lets this skill create the key and verify data for them: install `cx` (`brew install cx`, or `curl -fsSL https://get.coralogix.dev/cli | sh`), then the user runs `cx init --signup --region <region>`. It opens the signup page in their browser and waits (up to 30 minutes) until the new team exists. New accounts can pick `us2`, `us3`, `eu2`, `ap1`, `ap2` or `ap3`; the region is permanent, so let the user choose. Then continue with option 1.

   Don't wait idly: while the user signs up, do steps 3–5 with the key read from an env variable, and fill in the value at the end.

### 3. Install the SDK and add init

Follow the platform reference for the package, entry point and env-var naming. Every init sets:

| Option | Value |
|---|---|
| `application` | The app name (from `package.json` `name`, app id, or ask). Must exactly match `-a` in the source-map upload. |
| `version` | The release identifier, injected at build time: the git short SHA in CI, falling back to the `package.json` version locally. Must exactly match `-v` in the source-map upload. |
| `environment` | From the build mode / env (`production`, `staging`, `development`). |
| `public_key` | From env/config, never a string literal in source. |
| `coralogixDomain` | The region from step 2, upper case. |

If the region isn't known yet (the user is still signing up), read it from env the same way as the key; never fall back to a guessed region. Initialize only when the key (and region, if from env) is set, so the app runs unchanged without them.

Rules: init **once**, as early as possible, **client-side only**; add the env var to the existing `.env.example` (or create one) with an empty value, put the real value in the git-ignored local env file (`.env.local`), and check that file is git-ignored.

### 4. Privacy defaults

- Mask more interaction inputs than the SDK default: `maskInputTypes: ['password', 'email', 'tel']`.
- **Session Replay is off unless the user opts in.** Ask once: "Do you want Session Replay (records the page so you can watch a session back)?" If yes, enable it with all inputs masked (`maskAllInputs: true`) and ask whether to also mask all page text (`maskTextSelector: '*'`). See `references/browser.md` for the block, sample rate and CSP needs.
- Don't add `user_context` with personal data unless the user asks; if they do, prefer an internal user id over email.

### 5. Source-map upload

Minified stack traces are unreadable without this. Follow `references/source-maps.md`:

1. Make the production build emit source maps without exposing them publicly (hidden maps where the bundler supports it).
2. Add an upload step that runs **after** the production build: `coralogix-rum-cli upload-source-maps` (package `@coralogix/rum-cli`) with the same `application` and `version` as the init, and the region as `-e`.
3. Put it in CI if the repo has CI (GitHub Actions, GitLab CI, ...), reading the key from a CI secret named `CORALOGIX_SOURCEMAP_API_KEY`. Otherwise add a package script and tell the user how to run it.
4. Tell the user to create that **separate, secret** key in Coralogix (**Settings → API Keys**, a team key with the **`SourceMapping`** preset) and add it as a CI secret. Never reuse the RUM public key for this.

### 6. Build and verify

1. **Build the app** with the project's own build command. Fix anything the change broke before moving on.
2. **Check for leaked secrets:** `git status` / `git diff` must not contain the source-map key or a filled-in `.env.local`.
3. **Confirm data arrives** — the SDK sends in batches after a real page load:
   - With `cx`: have the user run the app and open it (or run it yourself if you can open a browser), then query:
     ```bash
     cx dataprime query --source rum.events \
       "filter \$d.version_metadata.app_name == '<application>' | limit 10" -p <profile> -o json
     ```
   - Without `cx`: tell the user to open the app, then in Coralogix go to **RUM → Sessions** (sessions appear within a few minutes) and **RUM → Error Tracking** (errors).
   - No data? Check the browser network tab for requests to `ingress.<region>.rum-ingress-coralogix.com`: ad blockers, a wrong `coralogixDomain`, a key without the `SendRUMData` preset, or a strict Content-Security-Policy are the usual causes.

Setup is **done** when: the SDK is installed and initialized at the entry point, the app builds, the source-map upload step is configured, no secret is in the diff, and the user has either seen data or knows exactly where to look.

### 7. Next steps (suggest, don't run)

- **Alerts** on new errors or error-rate spikes (the `cx-alerts` skill, or Coralogix → Alerts).
- **Fix with AI:** each error in RUM → Error Tracking has a "Fix with AI" button that copies a prompt for this coding agent; with source maps uploaded, `cx rum stacktrace` maps frames back to source.
- **Connect this agent to Coralogix** so it can query RUM data directly: the `cx` CLI (`cx init`) or the Coralogix MCP server (`https://api.<region>.coralogix.com/mgmt/api/v1/mcp`).

## Existing setup

If `@coralogix/browser` (or a mobile SDK) is already initialized: don't add a second init. Check it against step 3–5 (version wired to the release? source maps uploaded? key from env?) and report the gaps, fixing only what the user agrees to.

## Related Skills

- **`cx-telemetry-querying`** - query `rum.events` once data flows, map stack traces with `cx rum stacktrace`
- **`cx-alerts`** - alert on RUM errors
- **`cx-platform-admin`** - general API key management
- **`coralogix-docs`** - fetch the current SDK docs for a platform

## Reference files

- **`references/browser.md`** - browser SDK install, init per framework (Vite/React, Angular, Vue, Next.js, SvelteKit, CDN), env-var naming, Session Replay block
- **`references/mobile.md`** - React Native, Flutter, iOS and Android packages, entry points and docs to fetch
- **`references/source-maps.md`** - hidden source maps per bundler and the upload step for web and mobile
- **`references/coralogix-cli.md`** - using `cx` to pick a profile and reuse or create the RUM key
