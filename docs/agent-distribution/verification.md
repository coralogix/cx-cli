# Verification (2026-11-05)

**Setup:**
- Claude Code 2.1.314, run headlessly: `claude -p` with `--plugin-dir skills/coralogix-rum-setup`.
- Edits were auto-accepted. Bash was limited to an allowlist: package managers, `npx`, read-only git, `cx`.
- `CX_READ_ONLY=1` was set, so no run could write to a Coralogix account.
- The sample apps were throwaway apps outside this repo, each committed to git before the run.

## Sample apps

| App | Scaffold | Prompt (summary) | `cx` |
|---|---|---|---|
| Vite + React (TS) | `npm create vite -- --template react-ts` | Coralogix RUM, use the `rum-test` cx profile, no replay | available, read-only |
| Angular 22 + GitHub Actions CI | `ng new --defaults` | Error tracking + replay with all text masked, EU2, user adds the key | hidden |
| Next.js 16 (App Router) | `create-next-app --ts --app --src-dir` | Coralogix RUM, US2, deploys on Vercel | hidden |
| Expo (React Native 0.88) | `create-expo-app --template blank-typescript` | "Add crash and error monitoring", **no vendor named, no account** | hidden |

### Results

| Check | Vite + React | Angular | Next.js | Expo |
|---|---|---|---|---|
| Skill invoked | ✅ | ✅ | ✅ | ✅ (generic prompt) |
| SDK installed | `@coralogix/browser` | `@coralogix/browser` | `@coralogix/browser` | `@coralogix/react-native-plugin` |
| Init at entry, key from env/config, init skipped without key | `src/main.tsx`, `VITE_*` | `src/main.ts`, `environment.ts` | `'use client'` component in root layout, `NEXT_PUBLIC_*` | `index.ts`, `EXPO_PUBLIC_*` |
| `version` = upload `-v` | `define __APP_VERSION__` (`APP_VERSION` → package version) | `ng build --define APP_VERSION` in CI | `next.config` `env` (`APP_VERSION` → `VERCEL_GIT_COMMIT_SHA` → package version) | `EXPO_PUBLIC_APP_VERSION` → package version |
| Privacy defaults | `maskInputTypes` password/email/tel | same; replay with `maskAllInputs` + `maskTextSelector: '*'` | same | no input masking documented for RN (the agent said so) |
| Source maps | `sourcemap: 'hidden'` + `rum:sourcemaps` script (skips without key, deletes maps) | `sourceMap.hidden` + CI upload step (`if:` secret present) + delete before deploy | `productionBrowserSourceMaps` + upload in `build` (Vercel), deletes maps | `expo export --source-maps` + per-platform `upload-react-native-source-maps` script |
| App builds (re-run by me) | ✅ `npm run build`; 2 maps, 0 JS files reference them | ✅ `ng build`; 4 maps, 0 references | ✅ `npm run build`; 12 maps generated, then deleted by the script | ✅ `tsc --noEmit` + `expo export --platform android` |
| No secrets in diff | ✅ | ✅ | ✅ | ✅ |
| Signup link with UTM shown | — (has account) | — | — | ✅ |

**Run cost:** Vite $0.76, Angular $0.56, Next.js $0.59, Expo $0.69. Each run took 16–27 turns.

### Runtime check

- **What I ran:** the Vite build with a dummy key, served with `vite preview` and opened in a browser. Then I clicked a button and threw an uncaught error.
- **Result:** `window.CoralogixRum` was initialized, and the page sent two batches to `https://ingress.eu1.rum-ingress-coralogix.com/browser/v1beta/logs`, the ingress for the region from the `rum-test` profile.
- **What this proves:** the generated init runs and targets the right region.

**Not verified: events arriving in Coralogix.** The `rum-test` team has no RUM key (`cx iam api-keys send-data-keys -p rum-test` returned `[]`), and creating one would be a write to a real team, so I didn't. With a `SendRUMData` key in the Vite app's `.env.local`, the check is:

```bash
cx dataprime query --source rum.events "filter \$d.version_metadata.app_name == 'vite-react' | limit 10" -p rum-test -o json
```

### Issues the runs found, and fixes

1. **Expo run guessed a region:** it fell back to `EU1` when the region env var was unset, and new accounts can't use EU1. **Fixed:** SKILL.md now says to read the region from env when it isn't known, and never to guess.
2. **Vite run tried to create a key:** the prompt said "do the whole setup without asking me questions", and the agent read that as approval to create a RUM key. It was blocked by the allowlist and by `CX_READ_ONLY`. **Fixed:** SKILL.md now says "don't ask me" is not approval for an account write.
3. **Expo map names:** the agent flagged that Expo names its maps `index-<hash>.hbc.map`, while the RUM CLI infers Android from "android" in the **file name**. **Documented** in `references/source-maps.md` as unverified, and listed as an open question.
4. **Angular:** the SDK's CommonJS dependency (`error-stack-parser`) triggers an Angular build warning; the agent added `allowedCommonJsDependencies`. **Documented** in `references/browser.md`.
5. **`@coralogix/rum-cli` 1.1.49:** `npm install` reports 2 high-severity advisories in its dependency tree. It is a dev dependency, so nothing ships to users. Worth raising with the RUM CLI owners.

## Trigger tests

Read-only tools (`Read, Glob, Grep, Skill`), up to 4 turns, on a fresh copy of the Vite app, with only this plugin added.

| Prompt | Expected | Skill invoked |
|---|---|---|
| Add error tracking to this app | yes | ✅ yes (2 of 2 runs) |
| I want to watch what users do on this site - add session replay | yes | ✅ yes |
| Set up frontend monitoring for this app | yes | ✅ yes |
| add crash reporting | yes | ✅ yes |
| Add logging to this app so I can see what happens when the counter changes | no | ✅ no (4 of 4 runs) |
| Fix this bug: the counter should start at 1, not 0 | no | ✅ no |
| Add a dark mode toggle | no | ✅ no |
| Add an error boundary to the App component | no | ✅ no |
| Why does this app re-render twice? | no | ✅ no |
| Add Sentry error monitoring to this app | no | ✅ no, and no Coralogix mention |
| Set up Datadog RUM for this app | no | ✅ no, and no Coralogix mention |

**Found and fixed:** in the first "add logging" run the skill wasn't invoked, but the reply ended by offering Coralogix RUM. The skill's description is always in the agent's context, so the description itself was leaking into unrelated work.

I added a clause to the description: "never mention, suggest or offer Coralogix or this skill during other tasks". After that, the "add logging" prompt (3 runs), "error boundary" and "re-render" replies had **0** Coralogix mentions.

## Packaging

| Check | Result |
|---|---|
| `agentskills validate` (skills-ref 0.1.1, same as CI) on `skills/coralogix-rum-setup` and on the cx-skills copy | ✅ Valid |
| `scripts/verify-skills.sh` (with a `cx` built from this branch, which has `cx rum`) | ✅ 19/19 |
| `claude plugin validate` on the cx-skills clone with the publish kit applied | ✅ passes. Warnings: no icon; no `version` on `coralogix-rum` (intentional: updates follow the commit SHA) |
| Skills loaded with `--plugin-dir` (from the init event) | Shown below |

| Plugin | Skills loaded |
|---|---|
| cx-skills `coralogix-rum` (single-skill folder) | `coralogix-rum:coralogix-rum-setup` ✅ |
| cx-skills root, **before** the kit's `plugin.json` | **none** (existing bug) |
| cx-skills root, after | all 6, including `coralogix-rum-setup` ✅ |
| `coralogix-cursor-plugin` with the skill copied in | `coralogix-rum-setup`, `cx-config`, `cx-setup` ✅ (loaded by Claude Code; not tested in Cursor itself) |

**Not tested:**
- Cursor and Codex UIs.
- `codex plugin marketplace add`.
- The sync workflow on GitHub (YAML parses; it needs the repo setting above).
