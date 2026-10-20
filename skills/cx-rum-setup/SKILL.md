---
name: cx-rum-setup
description: >
  Use this skill when the user asks to "add RUM", "set up RUM", "install the RUM SDK",
  "add Coralogix RUM to my app", "create a RUM API key", "RUM public key",
  "init Coralogix RUM", "CoralogixRum.init", "add the browser SDK", "real user monitoring",
  "frontend monitoring", "monitor my web app", "session replay", "track frontend errors",
  "add RUM to React", "add RUM to Next.js", "add RUM to my mobile app",
  or wants to instrument a browser or mobile application with Coralogix Real User Monitoring
  end to end - RUM API key, SDK install, init code, and verifying data arrives.
metadata:
  version: "0.1.0"
---

# RUM Setup Skill

Use this skill to onboard an application to Coralogix Real User Monitoring (RUM): get a RUM API key, install the platform SDK, add the `init` call, and verify events land in the `rum.events` dataset.

---

## CLI Commands

| Command | Purpose |
|---|---|
| `cx iam api-keys list -o json` | Find an existing RUM key and the team ID |
| `cx iam api-keys send-data-keys -o json` | List send-data keys (RUM keys included) |
| `cx iam api-keys get <id> -o json` | Read a key's value |
| `cx iam api-keys create --from-file <file>` | Create the RUM key (write - needs approval + `--yes`) |
| `cx docs search <query>` / `cx docs fetch <suffix>` | Current SDK install and init docs per platform |
| `cx dataprime query --source rum.events '<query>'` | Verify RUM events arrive |
| `cx profiles list` | Profiles and regions - pick the active one for both key and `coralogixDomain` |

All commands accept `-p <profile>` to target a specific team.

---

## Workflow

### 1. Detect the platform

Inspect the user's repo before anything else:

| Signal | Platform |
|---|---|
| `package.json` with `next` | Next.js (browser SDK, client-only init) |
| `package.json` with `react`, `vue`, `@angular/core`, `svelte`, or no framework | Browser SDK (`@coralogix/browser`) |
| `package.json` with `react-native` | React Native SDK |
| `Podfile` / `Package.swift` / `*.xcodeproj` | iOS SDK |
| `build.gradle(.kts)` with Android plugin | Android SDK |
| `pubspec.yaml` | Flutter SDK |

### 2. Get a RUM API key

RUM keys are **public by design** - they ship inside client code. Create it with the **`SendRUMData` preset** - the same key the UI's RUM flow creates. That preset grants exactly one permission, `rum-ingress:SendData`, and is standalone: never add other presets or permissions. (Granting `rum-ingress:SendData` directly also works but shows as a `CUSTOM` key in the UI, so prefer the preset.) In Settings → API Keys a RUM key shows type "Send Your Data" with preset `SendRUMData`. Do **not** pick the `SendData` preset - it grants logs/metrics/spans ingest but not RUM, and makes the key secret, not public.

**Reuse first.** Check for an existing RUM key:

```bash
cx iam api-keys list -o json
cx iam api-keys send-data-keys -o json
```

Look for a key whose only preset is `SendRUMData` (or whose only permission is `rum-ingress:SendData`). Use `cx iam api-keys get <id> -o json` to read its value.

**Create one if none exists.** This is a write - describe it to the user and get approval before passing `--yes` (same rules as `cx-platform-admin`).

```bash
cat > rum-key.json <<'EOF'
{
  "name": "<app-name> RUM",
  "owner": { "teamId": <team-id> },
  "keyPermissions": { "presets": ["SendRUMData"], "permissions": [] },
  "hashed": false
}
EOF
cx iam api-keys create --from-file rum-key.json --yes -o json
```

- `teamId`: read `cached_team_id` from the profile file `~/.cx/profiles/<profile>.toml` (written by `cx init`), or the `owner.teamId` of any key in `cx iam api-keys list -o json`, or ask the user.
- `Permission denied` on `send-data-keys` or `create` means the user's role lacks team API-key management (`TEAM-CUSTOM-API-KEYS:MANAGE`). Don't retry with other payloads - ask the user to create the key in **Settings → API Keys** with only the `SendRUMData` preset or have an admin do it, then continue from step 3 with the key value.
- `hashed: false` keeps the key value retrievable later - fine for a public RUM key.
- The response contains `value` - that is the `public_key` for the SDK.

### 3. Fetch the current SDK docs

SDK package names, versions, and options change - fetch them instead of guessing (see `coralogix-docs`):

```bash
cx docs search "RUM SDK installation <platform>" --limit 5
cx docs fetch <suffix-from-search>
```

### 4. Install and initialize

Use **one profile for both the key and the domain**. The active profile is `-p <name>` if given, else `$CX_PROFILE` if set, else the `DEFAULT` row of `cx profiles list` - don't just read the `DEFAULT` row. If there are several profiles and none was named, ask the user which team the app belongs to, then pass `-p <name>` to every `cx` call.

`coralogixDomain` is that profile's `REGION` in upper case: `EU1`, `EU2`, `US1`, `US2`, `US3`, `AP1`, `AP2`, `AP3`.

Browser example (confirm against the fetched docs):

```bash
npm install @coralogix/browser
```

```ts
import { CoralogixRum } from '@coralogix/browser';

CoralogixRum.init({
  application: '<app-name>',
  environment: 'production',
  public_key: process.env.NEXT_PUBLIC_CX_RUM_KEY!, // or import.meta.env.VITE_CX_RUM_KEY
  coralogixDomain: 'EU1',
  version: '<app-version>',
});
```

Gotchas:

- **Init once, as early as possible** - the app entry point (`main.ts`, `index.tsx`, root layout).
- **Next.js:** init only on the client - a `'use client'` component that calls `init` in `useEffect`, rendered from the root layout. Never in server components.
- **Env vars:** keep the key in env config (`NEXT_PUBLIC_*`, `VITE_*`, `REACT_APP_*`), not hard-coded. It is public, but env config makes rotation easy.
- **`version`** should match the release - source maps and release comparison rely on it.
- **Source maps** (optional) need a *separate* key with the `SourceMapping` preset - never reuse the RUM key. See `cx docs search "RUM source maps"`.

### 5. Verify data arrives (optional)

Setup is done once the key exists and `init` is in place - data only flows after the app runs and users hit it, and the SDK sends in batches. Give the user this query to run later, or run it now if they load the app:

```bash
cx dataprime query --source rum.events \
  "filter \$d.version_metadata.app_name == '<app-name>' | limit 10" -o json
```

No rows? Check the browser network tab for requests to the RUM ingress endpoint. Ad blockers, a wrong `coralogixDomain`, or a key with extra permissions are the usual causes.

---

## Key Principles

- **RUM key = `SendRUMData` preset only** - nothing else on the key
- **Reuse before create**, and never pass `--yes` without user approval
- **Fetch SDK docs per platform** with `cx docs` - don't hard-code versions or options from memory
- **Client-side init only**, once, at the app entry point
- **Done = key + init in place** - hand over the `rum.events` query for checking data later

---

## Related Skills

- **`cx-platform-admin`** - general API key management
- **`coralogix-docs`** - fetch platform-specific SDK docs
- **`cx-telemetry-querying`** - query RUM data once it flows
