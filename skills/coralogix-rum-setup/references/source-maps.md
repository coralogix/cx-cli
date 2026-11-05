# Source-map upload

Coralogix resolves minified stack frames with source maps (web), Metro maps (React Native), ProGuard/R8 mappings (Android), dSYMs (iOS) and Dart symbols (Flutter). **Nothing uploads automatically:** the official `coralogix-rum-cli` (npm package **`@coralogix/rum-cli`**) is the only upload path. Docs: https://coralogix.com/docs/user-guides/rum/cli/rum-cli/

## The contract

- `-a` and `-v` must be **exactly** the SDK's `application` and `version`, or frames won't resolve.
- `-e` is the **Coralogix region** (`EU1`, `EU2`, `US1`, `US2`, `US3`, `AP1`, `AP2`, `AP3`), not the app environment.
- `-k` is a **secret** team key with the **`SourceMapping`** preset, read from `CORALOGIX_SOURCEMAP_API_KEY`. Never the RUM public key, never committed, never in a client env var (`VITE_*`, `NEXT_PUBLIC_*`, ...).
- An app+version pair is a release. Uploading the same pair again fails unless you pass `--override` (replace) — add it so CI re-runs of the same commit don't fail.
- Upload from CI (or a release script), not from developer machines.

Install it as a dev dependency so the version is pinned: `npm install -D @coralogix/rum-cli` (or the project's package manager). Before writing a mobile command, run `npx coralogix-rum-cli <command> --help` and use the flags it prints.

## Web: emit maps without publishing them

| Bundler | Production config | Maps land in |
|---|---|---|
| **Vite** (incl. SvelteKit, Vue, React) | `build: { sourcemap: 'hidden' }` in `vite.config.*` | `dist/assets/*.js.map` |
| **webpack** | `devtool: 'hidden-source-map'` for production | the `output.path` folder |
| **Angular CLI** | in `angular.json` → `projects.<app>.architect.build.configurations.production`: `"sourceMap": { "scripts": true, "styles": false, "vendor": false, "hidden": true }` | `dist/<project>/browser` |
| **Next.js** | `productionBrowserSourceMaps: true` in `next.config.*` (Next.js has no hidden mode) | `.next/static/chunks` |
| **CRA** | maps are on by default (`GENERATE_SOURCEMAP`) | `build/static/js` |

"Hidden" maps are generated without the `//# sourceMappingURL` comment, so browsers don't fetch them, but the `.map` files are still in the output folder. **Delete them after upload and before deploy:** `find <folder> -name '*.map' -delete`. Only files ending in `.js.map` are uploaded.

## Web: the upload command

```bash
npx coralogix-rum-cli upload-source-maps \
  -k "$CORALOGIX_SOURCEMAP_API_KEY" \
  -a <application> \
  -v "$APP_VERSION" \
  -f <maps folder> \
  -e <REGION> \
  --override
```

Where it goes depends on who builds what gets deployed:

1. **The repo's CI builds the deploy artifact** (GitHub Actions, GitLab CI, CircleCI, ...): add a step after the production build and before deploy. GitHub Actions example:

   ```yaml
   env:
     APP_VERSION: ${{ github.sha }}               # or a short SHA; same value for build and upload
     CORALOGIX_SOURCEMAP_API_KEY: ${{ secrets.CORALOGIX_SOURCEMAP_API_KEY }}
   # ...
       - run: npm run build
       - name: Upload source maps to Coralogix
         if: env.CORALOGIX_SOURCEMAP_API_KEY != ''
         run: |
           npx coralogix-rum-cli upload-source-maps -k "$CORALOGIX_SOURCEMAP_API_KEY" \
             -a <application> -v "$APP_VERSION" -f <maps folder> -e <REGION> --override
           find <maps folder> -name '*.map' -delete
   ```

   The `if` keeps forks and PRs without the secret green.

2. **A hosting platform builds from the repo** (Vercel, Netlify, Cloudflare Pages, Amplify): add a package script that runs after the build and skips cleanly without the key, and make it part of the platform's build command:

   ```json
   "scripts": {
     "rum:sourcemaps": "[ -z \"$CORALOGIX_SOURCEMAP_API_KEY\" ] && echo 'skip source maps: no key' || (coralogix-rum-cli upload-source-maps -k \"$CORALOGIX_SOURCEMAP_API_KEY\" -a <application> -v \"$APP_VERSION\" -f <maps folder> -e <REGION> --override && find <maps folder> -name '*.map' -delete)"
   }
   ```

   Tell the user to set `CORALOGIX_SOURCEMAP_API_KEY` (secret) and `APP_VERSION` in the platform's environment settings. Vercel exposes `VERCEL_GIT_COMMIT_SHA` and Netlify `COMMIT_REF`, which can feed `APP_VERSION`.

3. **No CI and no platform build:** add the `rum:sourcemaps` script only and tell the user to run it after each production build, with the key in their shell environment.

Don't run the upload yourself unless the user has the key set and asks you to.

## Mobile

| Platform | Produce | Upload (check `--help` first) |
|---|---|---|
| **React Native (Metro)** | Android: add `--sourcemap-output android/app/src/main/assets/index.android.bundle.map` to the `react-native bundle` step (or rely on the Gradle `react {}` config's map output). iOS: set `SOURCEMAP_FILE` in the "Bundle React Native code and images" Xcode build phase. | `coralogix-rum-cli upload-react-native-source-maps ...` — one run per platform; the OS is inferred from `android` in the file name, so don't rename the Android map. |
| **React Native with Expo** | `npx expo export --source-maps` writes maps under `dist/_expo/static/js/<platform>/` with hashed names (no `android` in the file name); EAS Build produces its own maps during the native build | Same command, per platform. Whether the CLI detects Android from the folder path is not documented — tell the user to check the first Android upload resolves frames. |
| **Android** (R8/ProGuard) | Release builds with `minifyEnabled true` write `app/build/outputs/mapping/<variant>/mapping.txt` | `coralogix-rum-cli upload-proguard ... -f app/build/outputs/mapping/release/mapping.txt` — pass the **file**, not the folder. |
| **iOS** | dSYMs from the archive (`<App>.xcarchive/dSYMs`) with Debug Information Format = "DWARF with dSYM File" for Release | `coralogix-rum-cli upload-dsym ... -f <dSYMs folder>` |
| **Flutter** | `flutter build appbundle --release --obfuscate --split-debug-info=build/symbols` (same flags for `ipa`) | `coralogix-rum-cli upload-dart-symbols ...` — one `*.symbols` file per run; first plain, the rest with `--merge`; don't rename them. Needs `@coralogix/rum-cli` 1.1.44+. |

## Checking it worked

A zero exit code doesn't prove frames resolve. After the next production error: Coralogix → **RUM → Error Tracking** → pick an error from that version → **Stack Trace** tab should show real file names and lines. With `cx`, `cx rum source-maps list --application <application>` lists the releases that have maps.
