# Source Maps: Sentry / Datadog → Coralogix

Verified against `@coralogix/rum-cli` 1.1.49, `cx docs fetch user-guides/rum/cli/rum-cli.md`, `@sentry/*` bundler plugins 11.11.0, `@datadog/datadog-ci` docs (2026-11-05).

## How Coralogix source maps work

- **One upload path:** `coralogix-rum-cli upload-source-maps`, run after the build. The Coralogix bundler plugins (`@coralogix/vite-plugin`, `@coralogix/webpack-plugin`) do **not** upload - they only emit `cx-metadata.json` for micro-frontends (`supportMfe: true`). Don't add them as a source-map replacement.
- **Release = application + version.** `-a` and `-v` must equal `application` and `version` in `CoralogixRum.init`, or frames won't resolve.
- **File discovery:** the CLI walks `-f` recursively and uploads every file ending in `.js.map`, keyed by file name (directory dropped). Hashed bundle names keep these unique; two maps with the same file name in different folders collide.
- **No code injection.** There's no debug-ID step (`sentry-cli sourcemaps inject`) and no `--minified-path-prefix` - remove those, don't translate them.
- **Re-uploads:** the default fails if the release exists. `--override` replaces it; `--merge` appends (for multi-step builds).
- **Key:** a Team Key with the `SourceMapping` preset - **secret**, CI only. Not the public RUM key. The CLI reads it only from `-k`, so pass a CI secret: `-k "$CX_SOURCEMAP_KEY"`.
- **Region:** `-e` takes the team's region (`EU1`, `EU2`, `US1`, `US2`, `US3`, `AP1`, `AP2`, `AP3`) - the same value as `coralogixDomain`.
- **Limit:** 200MB per folder.

Install it as a dev dependency so the version is pinned in the lockfile:

```bash
npm install -D @coralogix/rum-cli      # binary: coralogix-rum-cli
```

```bash
npx coralogix-rum-cli upload-source-maps \
  -k "$CX_SOURCEMAP_KEY" -a "<application>" -v "$APP_VERSION" -f ./dist -e "$CX_RUM_REGION" \
  -c "$(git rev-parse HEAD)" -n "<repo>" -o "<org>"
```

`-c` / `-n` / `-o` (commit, repo, org) are optional and replace Sentry's `set-commits` / Datadog's git metadata.

## Tool mapping

| Source | What it did | Coralogix replacement |
|---|---|---|
| `@sentry/vite-plugin` (`sentryVitePlugin({...})` in `vite.config.*`) | Injected debug IDs, created release, uploaded maps, optionally deleted them | Remove plugin + import. Keep/enable `build.sourcemap`. Add a CI upload step |
| `@sentry/webpack-plugin` (`sentryWebpackPlugin`) | Same, for webpack (also CRA/craco, Angular custom-webpack) | Remove plugin. Keep/enable `devtool`. CI upload step |
| `@sentry/esbuild-plugin`, `@sentry/rollup-plugin` | Same | Remove plugin. Keep `sourcemap`. CI upload step |
| `withSentryConfig(nextConfig, {...})` in `next.config.*` | Wrapped config: generated hidden client maps, uploaded, deleted them (`deleteSourcemapsAfterUpload`, default true), plus server instrumentation | **Server Sentry kept (default):** keep the wrapper (it instruments the server) but set `sourcemaps: { deleteSourcemapsAfterUpload: false }` so the client maps survive for the Coralogix upload; delete them yourself afterwards. **Server Sentry removed (user chose it):** unwrap to plain `nextConfig` and set `productionBrowserSourceMaps: true`. Either way: upload `.next/static`, then delete its `.map` files |
| `sentry-cli sourcemaps inject <dir>` | Debug-ID injection | Remove - not needed |
| `sentry-cli sourcemaps upload` / `releases files … upload-sourcemaps` | Upload | `coralogix-rum-cli upload-source-maps -f <dir>` |
| `sentry-cli releases new/finalize/set-commits` | Release bookkeeping | Remove; pass `-v` (release) and `-c/-n/-o` (commits) to the upload |
| `@sentry/wizard` artifacts: `.sentryclirc`, `sentry.properties`, `.env.sentry-build-plugin` | CLI/plugin config | List for the user to delete; never print their contents |
| `datadog-ci sourcemaps upload <dir> --service S --release-version V --minified-path-prefix P` | Upload | `coralogix-rum-cli upload-source-maps -f <dir> -a S -v V` (drop `--minified-path-prefix`; datadog-ci's git metadata / `--repository-url` → `-c`/`-n`/`-o`) |
| `@datadog/datadog-ci` devDependency, `datadog-ci.json` | CLI + config | Replace with `@coralogix/rum-cli` |

## Make sure the build still emits maps

Sentry's bundler plugins can switch source-map generation on by themselves. After removing a plugin, **build and check that `.js.map` files still appear**; if they don't, enable them explicitly. Prefer *hidden* maps (no `//# sourceMappingURL` comment) so browsers don't fetch them:

| Bundler | Setting |
|---|---|
| Vite | `build: { sourcemap: 'hidden' }` in `vite.config.*` |
| webpack | `devtool: 'hidden-source-map'` (production config) |
| esbuild | `sourcemap: 'external'` |
| Rollup | `output.sourcemap: 'hidden'` |
| Angular CLI | `angular.json` → `configurations.production.sourceMap`: `{ "scripts": true, "styles": false, "hidden": true }` |
| Next.js | `productionBrowserSourceMaps: true` in `next.config.*` (maps are public next to the JS - delete after upload) |
| Create React App | `GENERATE_SOURCEMAP=true` (default) |

If the source tool deleted maps after upload (Sentry `filesToDeleteAfterUpload`, Next.js `deleteSourcemapsAfterUpload`), keep that behavior: delete `.map` files **after** the Coralogix upload and before deploy:

```bash
find dist -name '*.map' -delete          # Next.js: find .next/static -name '*.map' -delete
```

## Where to put the upload

Run it in **CI after the build, before deploy**, with the key from the CI secret store. Don't put it in `npm run build` / `postbuild` unless the repo has no CI: a local build would then need the secret key.

GitHub Actions:

```yaml
      - name: Upload source maps to Coralogix
        run: >
          npx coralogix-rum-cli upload-source-maps
          -k "$CX_SOURCEMAP_KEY" -a my-app -v "$APP_VERSION" -f ./dist -e "$CX_RUM_REGION"
          -c "$GITHUB_SHA" -n "${{ github.event.repository.name }}" -o "${{ github.repository_owner }}"
        env:
          CX_SOURCEMAP_KEY: ${{ secrets.CX_SOURCEMAP_KEY }}
          CX_RUM_REGION: EU1
          APP_VERSION: ${{ github.sha }}
```

GitLab CI / CircleCI / others: same command in the job that builds, key from the platform's masked variable.

Folder per framework: Vite `dist`, Angular `dist/<project>/browser` (or `dist/<project>`), CRA `build`, Next.js `.next/static`, webpack `output.path`.

## Version: make init and upload agree

Sentry plugins auto-detect a release (git SHA) and inject it; Datadog requires `version` in init plus `--release-version`. Coralogix needs the **same string in two places**, so define it once:

- Vite: `define: { __APP_VERSION__: JSON.stringify(process.env.APP_VERSION ?? pkg.version) }`, or `VITE_APP_VERSION` env.
- Next.js: `NEXT_PUBLIC_APP_VERSION` set in CI and at build time.
- Angular: generate `environment.ts` `version` in CI, or use `package.json` version.

Reuse the release variable the repo already uses for Sentry/Datadog (`SENTRY_RELEASE`, `DD_VERSION`, `VITE_SENTRY_RELEASE`, …) as the source value where possible - that keeps release numbering continuous.

## Secrets and env vars

| Retire (list for the user, don't delete from CI yourself) | Add |
|---|---|
| `SENTRY_AUTH_TOKEN`, `SENTRY_ORG`, `SENTRY_PROJECT`, `SENTRY_URL`, `SENTRY_DSN`, `NEXT_PUBLIC_SENTRY_DSN`, `VITE_SENTRY_DSN` | `CX_SOURCEMAP_KEY` (secret, CI) |
| `DATADOG_API_KEY` / `DD_API_KEY`, `DATADOG_SITE` / `DD_SITE`, `DD_CLIENT_TOKEN`, `DD_APPLICATION_ID` (+ framework-prefixed variants) | RUM key env var (`VITE_CX_RUM_KEY`, `NEXT_PUBLIC_CX_RUM_KEY`, …) - public |
| | Region (`CX_RUM_REGION` or a constant) |

In `dual-run` mode retire nothing.

## Verify

```bash
cx rum source-maps list --application <application>      # release with the new version should be listed
cx rum stacktrace --application <application> --version <version> --frame '<minified-file>:<line>:<col>'
```

`cx rum` needs a cx build that includes it (`cx rum --help`); if missing, ask the user to update cx.
