# Verifying a RUM Migration

Run every check that applies. Report each as **passed**, **failed** (with output), or **skipped** (with the reason) - never claim a check you didn't run.

## 1. Static checks (always)

```bash
# Old SDK gone (replace mode) - expect no matches outside lockfiles/CHANGELOG
grep -rnE "@sentry/|sentry-cli|withSentryConfig|@datadog/browser-|datadog-ci|DD_RUM|datadogRum|datadogLogs" \
  --exclude-dir={node_modules,.git,dist,build,.next,coverage} .

# Coralogix wired in
grep -rn "CoralogixRum.init" --exclude-dir={node_modules,.git,dist,build,.next} .

# No secrets in the diff: no literal keys, DSNs or tokens
git diff | grep -nE "^\+.*(cxtp_|cxup_|dsn|https://[a-f0-9]+@|DD_API_KEY=|SENTRY_AUTH_TOKEN=|-k +['\"]?[A-Za-z0-9_]{16,})" || echo "no secrets in diff"
```

In `dual-run` mode the first grep is expected to match - check instead that both inits run and that the two SDKs don't double-wrap the same handler (see the mapping files).

- Lockfile updated with the same package manager the repo uses (`package-lock.json` / `pnpm-lock.yaml` / `yarn.lock` / `bun.lockb`). Never switch managers.
- `CoralogixRum.init` runs once, client-side only (Next.js: not in server components, `instrumentation.ts`, or server/edge config).
- The RUM key comes from env config, not a literal. The source-map key appears only as a CI secret reference.

## 2. Build and test (always, if the repo can build)

Use the repo's own scripts:

```bash
<pm> install
<pm> run build          # must pass
<pm> test               # if a test script exists; compare with the result before migrating
<pm> run lint           # if present
```

Run the build and tests **before** changing anything too, so pre-existing failures are not blamed on the migration.

After the build:

- `.js.map` files exist in the build output (`find dist -name '*.js.map' | head`) - otherwise source-map upload has nothing to send.
- If the old setup deleted maps after upload, the new pipeline does too.
- Type errors from removed Sentry/Datadog types (`Sentry.Event`, `RumEvent`, …) are fixed, not cast away with `any`.

## 3. Local smoke test (when the app can run locally)

1. Start the dev server (`<pm> run dev`) with the RUM key env var set. If the user has no key yet, skip and say so - `cx-rum-setup` step 3 gets one.
2. Load the app, and trigger a test error. Prefer an existing debug route; otherwise run in the browser console:
   ```js
   CoralogixRum.captureError(new Error('cx-rum-migrate smoke test'))
   ```
   (`window.CoralogixRum` exists only with the CDN build; with npm, add a temporary button or call it from an existing debug hook, and remove it afterwards.)
3. In the browser's network tab, confirm a POST to `ingress.<region>.rum-ingress-coralogix.com/browser/v1beta/logs` (or the `proxyUrl`) returns 2xx. Ad blockers can block it - test in a clean profile.

## 4. Data arrives in Coralogix (when `cx` is configured, or Coralogix MCP is connected)

Events are batched - allow a minute.

```bash
cx dataprime query --source rum.events \
  "filter \$d.version_metadata.app_name == '<application>' && \$d.event_context.type == 'error' | choose \$d.error_context.error_message, \$d.version_metadata.app_version, \$d.environment | limit 10" \
  --start now-15m -p <profile> -o json
```

Check: the smoke-test error is there, `app_version` equals the configured `version`, `environment` is right, labels and user context look like the old SDK's tags and user.

No `cx` and no MCP? Give the user this query to run later and mark the step skipped.

## 5. Source maps (when CI ran, or the user uploads manually)

```bash
cx rum source-maps list --application <application>
```

The release `<version>` should be listed. To prove frames resolve, take a frame from a real error event (`$d.error_context.original_stacktrace`) and run `cx rum stacktrace --application <application> --version <version> --frame '<file>:<line>:<col>'`. Don't upload from your machine with the user's secret key unless they ask.

## 6. Dual-run comparison (dual-run mode only)

Over the trial period, compare per day: error count, distinct issues (`rum_template_id` vs Sentry issues / Datadog error groups), sessions, and p75 LCP. Differences of a few percent are expected (different sampling, ad-block lists, grouping). Larger gaps usually mean sampling or `ignoreErrors`/`beforeSend` were not translated equivalently.
