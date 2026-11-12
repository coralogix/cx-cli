# Triage

Classify each error before mapping its stack. Map only "fixable here" and "library-only" errors that have an in-app caller.

| Class | Meaning | Action |
|---|---|---|
| **fixable here** | Root cause is in code in this repo | Continue to Phase 3 |
| **noise** | Not caused by the app, or not worth fixing | Skip; list with the reason |
| **library-only** | Every frame is in a dependency | Find the first in-app caller; if none, report |
| **backend-caused** | A failed request or bad response causes it | Report with evidence (`context.md`); don't patch the frontend |
| **config** | Deploy, cache, CSP, CDN or feature-flag setup | Report the config change needed |

## Noise

Skip, or flag as low priority:

- `ResizeObserver loop limit exceeded` / `ResizeObserver loop completed with undelivered notifications` - a browser warning, harmless.
- `Script error.` with no stack - a cross-origin script without CORS headers; there is nothing to map.
- Frames from `chrome-extension://`, `moz-extension://`, `safari-extension://` or `safari-web-extension://` - a browser extension, not the app.
- Bot or headless browsers: check the user agent.
- A single affected user (`users == 1`), unless the user asked for that error.

```bash
# User agents and browsers for one error
cx dataprime query --source rum.events "filter \$d.rum_template_id == '<template-id>' | groupby \$d.session_context.browser as browser, \$d.session_context.user_agent as user_agent aggregate count() as errors, distinct_count(\$d.session_context.user_id) as users | orderby errors desc" --start now-7d --limit 10 -o json
```

`HeadlessChrome`, `bot`, `crawler`, `spider`, `Lighthouse` or `PhantomJS` in the user agent means automated traffic.

## Library-only stacks

All mapped frames are under `node_modules/`, or have status `SOURCE_MAP_NOT_FOUND` (third-party scripts):

1. Walk the frames top-down and take the first frame whose `source.fileName` is in this repo (`git ls-files <path>`). That is where to look: usually the app passes bad input to the library or calls it at the wrong time.
2. No in-app frame: check breadcrumbs (`context.md`) for the user action that triggered it, and search the repo for where the library is called with that feature.
3. Still nothing: report as library-only, with the library name and version from the `node_modules/.pnpm/<name>@<version>` path. Suggest an upgrade only if its changelog mentions the error.

## ChunkLoadError / "Loading chunk failed" / "Failed to fetch dynamically imported module"

Usually right after a deploy: users with the old `index.html` request chunks that no longer exist. This is **config**, not code. Check:

- Does the error start at a version change? (impact by version, `context.md`)
- Are old chunks kept on the CDN for a while after a deploy?
- Is `index.html` served with `Cache-Control: no-cache`?

A code fix (reload the page once on chunk-load failure) is acceptable only if the user asks for it.

## Network, CORS and timeout errors

`type == 'network-request'`, `Failed to fetch`, `NetworkError`, `Load failed`, `timeout`, CORS messages:

1. Get the request's status codes and the linked backend trace (`context.md`).
2. 5xx or a backend error span → **backend-caused**. Report the service, operation and error.
3. Status `0` or no response, many browsers → network, CORS or an ad blocker. CORS → **config** (server headers).
4. 4xx caused by the request the frontend builds (bad params, missing auth refresh) → **fixable here**.

## Common frontend patterns

| Symptom | Usual root cause | Look at |
|---|---|---|
| `Cannot read properties of undefined (reading 'x')` / `undefined is not an object` | Data not loaded yet, an API response missing a field, or an object read before it was initialized | Where the object is assigned; the API contract; component init order |
| `Unhandled promise rejection` / `Uncaught (in promise)` | A promise with no `.catch` / `await` without `try`, often a failed request | The caller that drops the promise |
| Error in a lifecycle hook, `effect()`, `useEffect` or a subscription | Race: the code runs before a child, view, service or API is ready, or after it is destroyed | Angular: `ngOnInit` vs `ngAfterViewInit`, `effect()` reading signals before a view child exists, missing `takeUntilDestroyed`. React: effect cleanup, stale closures, state set after unmount |
| `x is not a function` / `x is not a constructor` | A lazily loaded module or polyfill is missing, loaded in a different order, or a dependency version changed its export | Lazy routes and dynamic imports, module load order, lockfile changes in the first-seen version |
| `Missing translation for ...` / raw i18n keys on screen | A key missing from a locale file, or a dynamic key built at runtime | Locale files; the code that builds the key |
| Error starts in one version | A regression in that release | `git log <previous-version-commit>..<first-seen-commit> -- <mapped files>` |
