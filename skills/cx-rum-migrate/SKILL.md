---
name: cx-rum-migrate
description: >
  Use this skill when the user asks to "migrate from Sentry", "migrate from Datadog RUM",
  "replace Sentry with Coralogix", "switch from Datadog to Coralogix RUM", "move off Sentry",
  "replace @sentry/react", "replace @datadog/browser-rum", "convert Sentry init to Coralogix",
  "move source maps from Sentry to Coralogix", "replace sentry-cli", "replace datadog-ci sourcemaps",
  "run Sentry and Coralogix side by side", "dual-run Coralogix RUM", "Sentry to Coralogix",
  "Datadog to Coralogix", or wants to move a frontend app's error tracking, RUM, session replay,
  or source-map upload from Sentry or Datadog to Coralogix RUM.
metadata:
  version: "0.1.0"
---

# RUM Migration Skill

Move a browser app from the Sentry or Datadog SDK to the Coralogix RUM SDK (`@coralogix/browser`): find every call site, map each feature to its Coralogix equivalent, swap the source-map upload, verify, and hand back one reviewable change.

**Scope:** browser apps - plain JS/TS, React, Angular, Vue, Next.js (client side). Mobile SDKs (React Native, Flutter, iOS, Android) are not covered yet: if the repo is mobile, say so and offer `cx-rum-setup` for a fresh Coralogix install instead.

---

## CLI Commands

| Command | Purpose |
|---|---|
| `cx profiles list` | Pick the team/region (`coralogixDomain`, `-e` for uploads) |
| `cx iam api-keys send-data-keys -o json` | Find a reusable RUM key (see `cx-rum-setup` step 3) |
| `cx docs search "<topic>"` / `cx docs fetch <suffix>` | Current Coralogix SDK docs |
| `cx rum source-maps list --application <app>` | Confirm a source-map release was uploaded |
| `cx rum stacktrace --application <app> --version <v> --frame <f:l:c>` | Prove frames resolve |
| `cx dataprime query --source rum.events '<query>'` | Confirm events arrive |

---

## Rules

- **Plan before editing.** Show the plan and get a go-ahead; no file changes before that.
- **Never present an unsupported feature as migrated.** Anything marked "No equivalent" in the mapping files goes in the report's gaps list, with what was done instead (removed, approximated, left as a TODO comment).
- **Never invent an API.** Use only options and calls listed in `references/rum-browser-init.md` and the mapping files. If the installed `@coralogix/browser` differs, check its `src/types.d.ts`.
- **Never commit secrets.** The RUM key goes through env config; the source-map key is a CI secret reference only. Don't print `.sentryclirc`, `sentry.properties`, `.env*` values.
- **Never remove old secrets or CI config silently.** List every Sentry/Datadog secret, env var, and CI step for the user; delete CI secrets only if they ask.
- **Privacy at least as strict as before.** Replay masking and `beforeSend` scrubbing must not loosen.
- **Neutral tone.** Describe differences factually; don't disparage other vendors.
- **No commit or PR unless asked.** Leave changes in the working tree; open a draft PR only on request.

---

## Workflow

### 1. Detect

Confirm it's a browser app (same detection order as `cx-rum-setup` step 1). Then inventory the source SDK - grep, excluding `node_modules`, build output, and lockfiles:

```bash
grep -rnE "@sentry/|Sentry\.|sentry-cli|withSentryConfig|sentryVitePlugin|sentryWebpackPlugin|SENTRY_" \
  --exclude-dir={node_modules,.git,dist,build,.next,coverage} .
grep -rnE "@datadog/browser-|datadogRum|datadogLogs|DD_RUM|DD_LOGS|datadog-ci|DD_|DATADOG_" \
  --exclude-dir={node_modules,.git,dist,build,.next,coverage} .
```

Collect:

| Item | Where to look |
|---|---|
| Packages + versions | `package.json` (all workspaces), lockfile |
| Init call(s) and every option | entry file, `sentry.*.config.*`, `instrumentation*.ts`, `datadog*.ts`, `index.html` CDN snippets |
| Runtime calls | `captureException`, `captureMessage`, `setUser`, `setTag(s)`, `setContext`, `setExtra`, `addBreadcrumb`, `withScope`; `addError`, `addAction`, `setUser`, `setGlobalContextProperty`, `addTiming`, logger calls |
| Framework integrations | `ErrorBoundary`, `withErrorBoundary`, Angular `ErrorHandler`/`TraceService`, Vue `app`, Next.js `global-error.tsx`, `@datadog/browser-rum-react` |
| Privacy markup | `sentry-mask`/`sentry-block`/`data-sentry-*`, `data-dd-privacy`, `dd-privacy-*` classes |
| Build plugins | `vite.config.*`, `webpack.config.*`, `next.config.*`, `angular.json`, esbuild/rollup configs |
| CI upload + secrets | `.github/workflows`, `.gitlab-ci.yml`, `.circleci`, `Jenkinsfile`, `bitbucket-pipelines.yml`, `.sentryclirc`, `sentry.properties`, `datadog-ci.json` |
| Env vars | `.env.example`, `.env*` (names only), `environment*.ts`, deployment manifests |
| Tests touching the SDK | mocks of `@sentry/*` / `@datadog/*` |

Both vendors present (e.g. Sentry errors + Datadog RUM)? Migrate both in one plan; both mapping files apply.

### 2. Plan - and stop for approval

Read the mapping file for the source SDK (`references/sentry-mapping.md` or `references/datadog-mapping.md`) and `references/sourcemaps.md`. Present:

1. **Files to change**, with one line each on what changes.
2. **Feature map** - every detected option/call → its Coralogix form (from the mapping tables).
3. **Gaps** - every detected feature with no equivalent, and what will happen to it.
4. **Settings to choose** - Coralogix team/region (`cx profiles list`), `application` name (default: `package.json` `name`), version source.
5. **Secrets and env vars** - new ones to add (RUM key env var, `CX_SOURCEMAP_KEY` CI secret) and old ones to retire.
6. **Mode** (step 3) and **rollback** (revert the change; in dual-run, delete the Coralogix init).

### 3. Choose a mode

| Mode | What happens | When |
|---|---|---|
| `replace` (default) | Remove the old **browser** SDK, its plugins, upload steps, and config; Coralogix only in the browser | Small/medium apps, or after a successful dual-run |
| `dual-run` | Keep the old SDK untouched; add Coralogix alongside; add the Coralogix upload step next to the old one | Large apps, or when the team wants to compare data first |

**Server-side SDK stays by default, in both modes.** The Coralogix browser SDK replaces only browser instrumentation. Leave server/edge code untouched: Next.js `sentry.server.config.*`, `sentry.edge.config.*`, `instrumentation.ts`, server wrappers, `withSentryConfig` (it also instruments the server), Node `@sentry/node` / `dd-trace`. In Next.js, `@sentry/nextjs` stays installed for the server and only the client init (`instrumentation-client.ts` / `sentry.client.config.ts`) moves to Coralogix. Remove server-side code only if the user explicitly chooses to in the plan, after you've told them server errors stop being reported until they set up Coralogix APM (OpenTelemetry).

Dual-run notes: init both SDKs at the same point; put Coralogix calls next to the old ones (e.g. call both `Sentry.captureException` and `CoralogixRum.captureError`) - a small wrapper module in the app is fine; keep both upload steps; trace propagation: send only one header family per backend unless the backend accepts both (see mapping files). Report what must be done to finish the switch later.

### 4. Migrate the code

Work from the mapping tables; for Coralogix init use `references/rum-browser-init.md`.

1. **Packages:** remove the source packages (replace mode) and add `@coralogix/browser` with the repo's package manager. Add `@coralogix/rum-cli` as a dev dependency for uploads.
2. **Init:** one `CoralogixRum.init` at the framework entry point (table in `rum-browser-init.md`). Carry over environment, release → `version`, sampling (convert rates - Coralogix uses 0–100 per session), ignore lists, `beforeSend` (rewrite against the Coralogix event shape), replay + masking, trace propagation, tunnel/proxy.
3. **Runtime calls:** rewrite every call site. Prefer direct `CoralogixRum.*` calls; add a thin wrapper only where the old code already had one.
4. **Framework integrations:** replace vendor error boundaries/handlers with the patterns in the mapping file.
5. **Privacy markup:** rewrite vendor mask/block classes and attributes to the Coralogix ones the init config uses.
6. **Tests:** update SDK mocks; don't delete tests.
7. **Env:** add the RUM key variable to `.env.example` (name only, empty value) and to typed env declarations (`vite-env.d.ts`, `environment.ts`).
8. **Unsupported features:** remove the call and leave a `// TODO(cx-rum-migrate): <feature> has no Coralogix equivalent - <what to do>` only where behavior is lost and someone must decide; list each in the report.

### 5. Replace the source-map step

Follow `references/sourcemaps.md`: remove the vendor bundler plugin / CLI step, make sure the build still emits `.js.map`, add `coralogix-rum-cli upload-source-maps` in CI after the build with `-a`/`-v` equal to init, key from a CI secret, and keep any delete-after-upload behavior.

### 6. Verify

Follow `references/verification.md`: static greps, install, build, tests (compare with the pre-migration baseline), source maps emitted, and - when possible - a local test error confirmed in `rum.events` via `cx` (or the Coralogix MCP server).

### 7. Report

Give the user:

- **Changed** - files and a one-line summary each.
- **Migrated** - feature map actually applied.
- **Not migrated** - each gap, why, and the suggested follow-up.
- **Verification** - each check: passed / failed (output) / skipped (reason).
- **Manual follow-ups** - secrets to add (`CX_SOURCEMAP_KEY`, RUM key env var in each environment), old secrets and CI steps to retire, alerts/dashboards to recreate (optional, separate phase: there is no importer for Sentry alert rules or Datadog monitors/dashboards - export them with the vendor's API if the user wants, then rebuild with `cx-alerts` / `cx-dashboards`), server-side Sentry/Datadog APM to move separately.
- **Rollback** - `git revert` of the change, or for dual-run, removing the Coralogix init and upload step.

---

## Key Principles

- **Inventory first, plan second, edit third** - no edits before the user approves the plan
- **Mapping tables are the source of truth** - not memory
- **Gaps are reported, never hidden**
- **`application` + `version` must match** between init and source-map upload
- **Secrets stay out of code** - RUM key via env, source-map key via CI secret

---

## Related Skills

- **`cx-rum-setup`** - create the RUM API key and the source-map key, fresh SDK install, mobile apps
- **`cx-telemetry-querying`** - query `rum.events` after the switch
- **`cx-alerts`** - recreate error alerts in Coralogix
- **`cx-dashboards`** - recreate RUM dashboards
- **`coralogix-docs`** - fetch current SDK docs

---

## Additional Resources

### Reference files

- **`references/sentry-mapping.md`** - Sentry browser SDKs → Coralogix: options, calls, replay, tracing, framework integrations, gaps
- **`references/datadog-mapping.md`** - Datadog browser RUM/logs → Coralogix: options, calls, privacy, tracing, gaps
- **`references/sourcemaps.md`** - sentry-cli / bundler plugins / datadog-ci → the Coralogix RUM CLI, per bundler and CI
- **`references/verification.md`** - checks to run after migrating
- **`references/rum-browser-init.md`** - Coralogix browser SDK init options and runtime API (shared with `cx-rum-setup`)
