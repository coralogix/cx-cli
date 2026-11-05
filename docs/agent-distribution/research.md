# Coralogix RUM through coding agents: research and design

Goal: when a developer asks a coding agent (Claude Code, Cursor, Codex, ...) to add error monitoring, RUM, session replay or frontend monitoring, a public Coralogix skill detects the stack, installs and configures the RUM SDK, wires source-map upload into the build, sends people without an account to signup, and confirms data arrives. Success = new RUM trials and customers from agents.

Researched 2026-11-05. Detailed, fully cited findings are in the appendix; this page summarizes them and gives the design.

- [appendix/coralogix-rum-docs.md](appendix/coralogix-rum-docs.md) — SDK packages, init options, source maps, trial, MCP (every claim cites a docs page, registry or URL)
- [appendix/distribution-channels.md](appendix/distribution-channels.md) — Claude Code, `npx skills`/skills.sh, Cursor, Codex, MCP registries
- [appendix/competitors.md](appendix/competitors.md) — Sentry `getsentry/sentry-for-ai` and Datadog `datadog-labs/agent-skills` teardown, pinned to commits

Coralogix docs are cited as `D:<path>`, meaning `https://coralogix.com/docs/<path without .md>/` (also readable with `cx docs fetch <path>`).

**Status labels:** **Confirmed** means read in an official source today. **Inferred** means deduced from code or page source, not stated anywhere. **Open** means not confirmed.

---

## Key findings

1. **The skill was already public, but useless without an account and the CLI.** `skills/cx-rum-setup` was installable by anyone with `npx skills add coralogix/cx-cli/skills`; it is listed at https://skills.sh/coralogix/cx-cli. But every step assumed an authenticated `cx` (key creation through `cx iam`, region from a `cx` profile), so a developer with no Coralogix account had no path through it.
2. **No new public repo is needed.** Coralogix already runs:
   - [`coralogix/cx-skills`](https://github.com/coralogix/cx-skills) (Apache-2.0): a multi-agent skills package with `.claude-plugin/` (marketplace `cx-skills`, plugin `coralogix`), `.codex-plugin/` and `.cursor-plugin/` manifests, published to Tessl. It holds the OpenTelemetry and AI-app instrumentation skills, the same "instrument my app for Coralogix" category as RUM setup.
   - [`coralogix/coralogix-cursor-plugin`](https://github.com/coralogix/coralogix-cursor-plugin): already listed in the Cursor Marketplace as "Coralogix".
   - The MCP server is already listed in the official MCP Registry as `com.coralogix/mcp-server`, and as an "Anthropic verified" connector at https://claude.com/connectors/coralogix.
3. **A self-serve trial exists:** 14 days, no credit card. The CLI can start it, with the user doing the browser part: `cx init --signup` opens the Coralogix signup page and creates a `cx` profile once the user's new team exists ([docs/agent-setup.md](../agent-setup.md), `src/oauth.rs`). After that, `cx` can create the RUM key (with the user's approval) and query `rum.events` to verify data.
4. **Source maps have no per-bundler guidance in the Coralogix docs.** The docs give the `coralogix-rum-cli` upload command only. The skill fills the gap with standard bundler settings (hidden maps), which are marked as our guidance, not Coralogix docs.
5. **Competitors:**
   - Sentry and Datadog both ship one skill source, with thin per-agent manifests or repos generated from it.
   - Sentry verifies end to end against its backend. Datadog stops at "the build passes".
   - Neither handles an existing other-vendor SDK, uses negative clauses in its trigger descriptions, or tags signup links for attribution.

---

## Step 1: Research

### 1.1 Our setup today (`skills/cx-rum-setup`, as of `cababaa`)

| Topic | What it did |
|---|---|
| **Detection** | An ordered check that stops at the first match: Flutter (`pubspec.yaml`), React Native (`react-native`/`expo`), Next.js, browser (`react-dom`, `vue`, `@angular/core`, `svelte`, `vite`, `webpack`, `parcel`, `index.html`), iOS (`Podfile`, `Package.swift`, `*.xcodeproj`), Android (Gradle Android plugin). |
| **Platform depth** | Only the browser had concrete code: one `CoralogixRum.init` example and a Next.js "client only" note. React, Angular and Vue were covered by the generic example. React Native, Flutter, iOS and Android were detected, then the agent was told to fetch docs with `cx docs`. |
| **Source maps** | One bullet: upload with `coralogix-rum-cli` using a separate `SourceMapping` key kept in CI, plus a pointer to the RUM CLI doc. There was no bundler config, no CI step, and no version contract beyond "should match". |
| **Assumptions** | An existing Coralogix account and an **authenticated `cx` profile**. The key came from `cx iam api-keys send-data-keys` (reuse) or `cx iam api-keys create` with the `SendRUMData` preset. The region came from the profile. Docs came from `cx docs`. Verification used `cx dataprime query --source rum.events`. |
| **Missing** | A no-account or no-`cx` path, privacy defaults, Session Replay, a build check, a secrets check, and handling of a competitor SDK. |

Related existing content:
- [`skills/shared/rum-querying.md`](../../skills/shared/rum-querying.md): querying `rum.events`, and `cx rum stacktrace` / `cx rum source-maps list` (PR #280) for mapping frames, used only when the user asks.
- [docs/agent-setup.md](../agent-setup.md): the "Connect agent" flow, including `cx init --signup`.

### 1.2 Coralogix RUM SDK and docs (Confirmed unless noted; details and citations in the appendix)

**Packages (latest today):**

| Platform | Package | Version |
|---|---|---|
| Browser | `@coralogix/browser` | 4.4.0 |
| Browser via CDN | `https://cdn.rum-ingress-coralogix.com/coralogix/browser/<version>/coralogix-browser-sdk.js`; the version must be pinned, since `latest` is no longer served (D:user-guides/rum/sdk-installation/javascript/cdn-browser.md) | — |
| React Native | `@coralogix/react-native-plugin` (replaces `@coralogix/react-native-sdk`) | 0.15.0 |
| Flutter | `cx_flutter_plugin` | 0.20.0 |
| iOS | SPM `coralogix/cx-ios-sdk` or CocoaPods `Coralogix` (+ `SessionReplay`) | 2.22.0 |
| Android | `com.coralogix:android-sdk` | 2.25.0 |
| Source-map CLI | `@coralogix/rum-cli` (binary `coralogix-rum-cli`) | 1.1.49 |

Sources: npm, pub.dev, CocoaPods trunk, Maven Central, D:user-guides/rum/sdk-installation/*.

**Framework pages:** Next.js and SvelteKit have dedicated pages. Angular, React and Vue are covered by the generic npm page. The Angular-relevant option is `runOutsideAngularZone`, default `true`. No React error-boundary integration is documented; `CoralogixRum.captureError` is the manual API.

**React Native and Expo:**
- **Confirmed (package contents):** `@coralogix/react-native-plugin` 0.15.0 ships native code (`CxSdk.podspec`, `android/build.gradle`) and no Expo config plugin.
- **Inferred:** it can't run in Expo Go and needs a development build.

**Browser init options:**

| Group | Options |
|---|---|
| Core | `public_key`, `application`, `version`, `environment`, `coralogixDomain` (`EU1`/`EU2`/`US1`/`US2`/`US3`/`AP1`/`AP2`/`AP3`; init aborts on an unknown value), or `coralogixDomainUrl` |
| Sampling | `sessionConfig.sessionSampleRate`, default 100 |
| Interaction masking | `maskInputTypes`, default `['password']` (narrowed from `password`/`email`/`tel` in a breaking change); `maskClass`, default `cx-mask` |
| Session Replay | **Opt-in**: `sessionRecordingConfig: { enable: true, autoStartSessionRecording: true, ... }`. Defaults are `maskAllInputs: false` and `maskInputOptions: { password: true }`, so it is **not** private by default. `maskTextSelector: '*'` masks all text. CSP needs `script-src https://cdnjs.cloudflare.com/ajax/libs/pako/` and a `connect-src` entry for the ingress host. |

Sources: D:user-guides/rum/sdk-installation/javascript/npm-browser.md, D:user-guides/rum/product-features/session-replay/web.md, `@coralogix/browser@4.4.0` types.

**Source maps (D:user-guides/rum/cli/rum-cli.md):**

The command is:

```
coralogix-rum-cli upload-source-maps -k <key> -a <application> -v <version> -f <folder> -e <REGION> [--override|--merge] [-c -n -o]
```

I verified the flags against the 1.1.49 package source. Rules:

- **Matching:** `-a` and `-v` must exactly match the SDK's `application` and `version`.
- **`-e` is the region**, not the environment.
- **Re-uploads:** re-uploading an existing app+version fails without `--override` or `--merge`.
- **Key:** a team key with the **SourceMapping** preset, which is secret. Among system roles, only Platform Admin has `SOURCE-MAPPING:UPLOADMAPPING`.
- **Only path:** "Nothing uploads automatically". The `@coralogix/vite-plugin` and `webpack-plugin` packages only write micro-frontend metadata; they don't upload.
- **Mobile commands:** `upload-react-native-source-maps`, `upload-proguard` (pass the file, not the folder), `upload-dsym`, and `upload-dart-symbols` (one file per run, `--merge` after the first).

**Per-bundler guidance:**
- **Open:** the Coralogix docs say only "point `-f` at whatever folder your bundler wrote them to". Nothing covers Vite, webpack, Angular CLI, Next.js or Xcode dSYM locations.
- The skill uses each bundler's documented settings: Vite `build.sourcemap: 'hidden'`, webpack `devtool: 'hidden-source-map'`, Angular `sourceMap.hidden`, Next.js `productionBrowserSourceMaps`. It also deletes `.map` files after upload.
- **Open:** how Coralogix matches uploaded maps to frames (by file name? by URL path?). This matters for Next.js `/_next/static/...` paths.

**Where users see data:** RUM → Error Tracking, RUM → Sessions, and Releases (which shows whether a release has source maps). Sources: D:user-guides/rum/getting-started/real-user-monitoring.md, D:user-guides/rum/product-features/user-sessions.md, D:user-guides/rum/product-features/releases.md.

### 1.3 Getting an account

| Question | Answer | Status |
|---|---|---|
| Self-serve trial? | Yes, **14 days, all features, no credit card**, with a quota of 8 units. https://coralogix.com/pricing/ | Confirmed |
| Free tier? | None is mentioned on the pricing page. | Confirmed (absence) |
| Signup URL | https://coralogix.com/freetrialsignup/ (linked from "Start Free Trial"). https://signup.coralogix.com/ redirects (301) to https://dashboard.eu2.coralogix.com/#/signup. | Confirmed |
| Requirements | A work email (from an error string in the signup page's script), reCAPTCHA, email verification, and creating a team. | Inferred from page source and `docs/agent-setup.md` |
| Regions open to signup | `us2`, `us3`, `eu2`, `ap1`, `ap2`, `ap3`. Not `us1` or `eu1`. | Inferred (signup-page regex) and matches `docs/agent-setup.md`; not in public docs |
| Can the CLI create a trial? | **The user creates it; the CLI starts and finishes the flow.** `cx init --signup --region <r>` opens the signup page (OAuth `prompt=create`), waits up to 30 minutes, and creates a `cx` profile once the team exists. The CLI never fills in the form. | Confirmed in code (`src/oauth.rs`, `src/main.rs`) and [docs/agent-setup.md](../agent-setup.md). **Not in the public coralogix.com docs.** |
| Can an API produce a RUM public key? | Yes. `ApiKeysService/CreateApiKey` with `presets: ["SendRUMData"]`, which is what `cx iam api-keys create` uses. The caller needs `DATA-INGEST-API-KEYS:MANAGE`. D:developer-portal/apis/data-management/send-your-data-management-api.md | Confirmed |
| A RUM onboarding wizard in the UI? | Not found in public docs. | Open |
| UI path for API keys | "Settings → Users and Teams → API Keys" in one page, "Data Flow → API Keys" in another. The skill says **Settings → API Keys**. | Docs disagree |

### 1.4 Distribution channels

| Channel | Mechanism and format | Own repo? | Who approves | Coralogix today |
|---|---|---|---|---|
| **`npx skills add`** (vercel-labs `skills`, about 40 agents) | Agent Skills `SKILL.md` folders. Spec: `name` matches the folder; `metadata` is string-to-string. https://agentskills.io/specification | No: `owner/repo/path` and `--skill` work | Nobody. skills.sh lists skills automatically from install telemetry (https://skills.sh/docs/faq). | `coralogix/cx-cli/skills` (about 64K installs, some stale names) and `coralogix/cx-skills` |
| **Claude Code, own marketplace** | `.claude-plugin/marketplace.json` (`name`, `owner.name`, `plugins[{name, source}]`). The plugin `source` can be `./subdir` or `git-subdir`. A folder with only a `SKILL.md` loads as a single-skill plugin. https://code.claude.com/docs/en/plugins/marketplace-reference.md | No | Nobody: "the plugin is published, with no submission form" (https://code.claude.com/docs/en/plugins/publish.md) | `cx-skills` marketplace, plugin `coralogix` |
| **Claude, Anthropic directory** (claude.ai, Cowork, Claude Code) | Submit a plugin bundle (a repo folder path works) at https://claude.ai/directory/manage. Needs a paid claude.ai plan and push access to the GitHub repo. | No | Anthropic: automated scan plus human review (https://claude.com/docs/plugins/submit) | Connector listed; plugin **not** listed |
| **Claude, `claude-plugins-official`** | No portal: "ask [your Anthropic partner contact]" (publish docs) | — | Anthropic | Not listed. Datadog, Sentry, Grafana, Honeycomb, New Relic and Dynatrace are. |
| **Cursor** | Native `SKILL.md` support. Distributed as plugins: `.cursor-plugin/plugin.json` with `skills/`, `rules/`, `mcp.json`. https://cursor.com/docs/plugins | Must be open source | Cursor: every plugin **and every update** is reviewed by hand. Submit at https://cursor.com/marketplace/publish. | `coralogix-cursor-plugin` listed (MCP + `cx-setup`/`cx-config` skills) |
| **Codex** | Skills from `.agents/skills` and `~/.agents/skills`. Plugins use a root `plugin.json` (or `.codex-plugin/plugin.json`) with `skills`. `openai/skills` is deprecated; `openai/plugins` is curated. https://developers.openai.com/plugins/build/plugins | No | OpenAI, for directory listing: ZIP upload at https://platform.openai.com/plugins after identity verification. Skills-only plugins need no MCP test cases. | `cx-skills` has `.codex-plugin/plugin.json`; not in the directory |
| **MCP registries** | Official registry `server.json` via `mcp-publisher`, with DNS namespace `com.coralogix/*`. The GitHub MCP Registry takes requests by email to partnerships@github.com. mcp.so has a form. PulseMCP submissions are paused. Smithery's proxy shut down on 2026-10-31. | — | Varies | Official registry: listed. Anthropic connectors: verified. GitHub, mcp.so: **Open**. |

### 1.5 Competitor teardown (patterns, not text)

| | Sentry (`getsentry/sentry-for-ai` @ 8b49e14) | Datadog (`datadog-labs/agent-skills` @ 6bd95cc) |
|---|---|---|
| **Distribution** | One source repo. Build scripts generate per-agent plugin repos (`plugin-claude`, `plugin-cursor`, `plugin-codex`, ...). `npx @sentry/agent-plugin` runs each agent's own install command. | A flat repo that is its own Claude marketplace (`source: "./"`). Separate repos are listed in Cursor and Claude official. |
| **Setup skill** | `sentry-get-started` and `sentry-instrument`, about 240 lines each. They fetch docs.sentry.io pages as markdown at runtime, using a table that maps marker files to docs URLs. | `dd-instrument-rum` (111 lines plus 7 references) does the work. `dd-browser-sdk` (44 lines) is mostly upgrades but claims setup in its description. The two overlap; skills.sh shows 625 and 64 installs. |
| **Detection** | Marker files, then the agent confirms the framework with the user. | Explicit precedence: monorepo, framework, package manager (the `packageManager` field, then the lockfile), bundler, entry point. Stops on conflicting setups and never overwrites a valid init. |
| **Account and keys** | Through its MCP: looks up the DSN and creates a project after the user says yes. With no account it links to signup, with no UTM. The agent never runs the wizard itself. | The app ID and client token are public values. A `CreateRumApplication` tool creates the app; it isn't defined in the repo (Open). A separate `dd-account-setup` skill does OAuth or creates the account in the terminal with `signup_source: "skill"`. |
| **Privacy** | Replay is not added by default. When added: mask all text, block media. | `defaultPrivacyLevel: mask-user-input`, replay sampled at 20%. |
| **Source maps** | Pushed as a production step, using Debug IDs and hidden maps. | The build plugin, or `datadog-ci` for Next.js. It builds without a key to prove maps are found, without uploading. |
| **After install** | Runs the app, triggers a tagged error, polls until it arrives, and shows the issue URL. | Build only; nothing checks that data arrived. |
| **Attribution** | `utm_source=plugin` on the MCP URL, plus an onboarding status ping. | `signup_source: "skill"` |

**Patterns to copy:**
- A thin SKILL.md with references loaded only when needed.
- Detection precedence that the agent confirms.
- Never overwrite an existing init.
- Smallest scope by default.
- Public and secret keys kept separate.
- Source maps as a build/CI step that is safe without the key.
- End-to-end verification when possible.
- One source for all agents.
- A UTM on the signup link.

**Gaps we can beat:**
- Their mobile coverage is weak.
- Datadog's two skills overlap.
- Neither is neutral when another vendor's SDK is already present.
- Neither has a "do not use when" clause.

### 1.6 Open questions

1. **Trial signup through agents:**
   - Does the trial page record UTM parameters on the lead (does marketing see `utm_source=coding-agent`)?
   - Does the 14-day, 8-unit trial apply in every signup region?
   - Is "work email required" enforced?
2. **CLI signup:** `cx init --signup` exists, but it isn't in the public docs and has no attribution parameter. Should signups via the CLI carry a source tag (e.g. `signup_source=cx-cli` on the OAuth `prompt=create` URL)? That needs a backend change.
3. **Account creation by API:** there is no public API to create an account or trial, and the skill must not create one anyway. The CLI can create the RUM key after the user signs up.
4. **Source-map matching:** how are frames matched to uploaded maps (file name or URL path)? This decides the right `-f` folder for Next.js and for apps served under a base path.
5. **A UI wizard for the RUM key:** is there one, and where does the UI show it? Two docs pages give different API-key paths.
6. **Expo:** is an Expo config plugin planned for `@coralogix/react-native-plugin`? Today Expo users need a development build (Inferred).
7. **Listings:**
   - What does an Anthropic plugin-directory listing need from brand and legal?
   - Who is Coralogix's Anthropic partner contact for `claude-plugins-official`?
   - Does `codex plugin marketplace add` honor `.claude-plugin/marketplace.json`?
   - What fields does the Cursor publish form ask for?
   - How can stale names be removed from skills.sh?
   - Is Coralogix listed in the GitHub MCP Registry and on mcp.so?

---

## Step 2: Design

### Where it lives: one source, many channels

- **Source of truth:** [`skills/coralogix-rum-setup/`](../../skills/coralogix-rum-setup/) in `coralogix/cx-cli`, renamed from `cx-rum-setup`. It is one skill that works **with or without `cx`**. With an authenticated `cx` it does the key reuse/create and data verification itself; without `cx`, it asks the user for the key and region, or gives a signup link. The CLI-specific steps moved to `references/coralogix-cli.md`, so the body reads correctly for someone who has never heard of `cx`.
- **CLI bundle:** unchanged. `cx init` / `cx skills install` install every folder under `skills/`.
- **Public packages:** built from that folder, not edited by hand:

| Channel | How it gets the skill | Ready? |
|---|---|---|
| `npx skills add coralogix/cx-cli/skills --skill coralogix-rum-setup` (Claude Code, Cursor, Codex and about 40 other agents) | Directly from this repo | **Ready on merge** |
| `coralogix/cx-skills` (`npx skills add coralogix/cx-skills`, Claude `coralogix@cx-skills`, Codex and Cursor manifests, Tessl) | A copy at `skills/rum/coralogix-rum-setup/`, kept in sync by a scheduled workflow in cx-skills that opens a PR when the cx-cli copy changes. Its marketplace gains a single-skill `coralogix-rum` plugin for a one-line install. | Needs a PR to cx-skills (prepared in [publish-kit/cx-skills/](publish-kit/cx-skills/); runbook in [publishing.md](publishing.md)) |
| Cursor Marketplace (`coralogix-cursor-plugin`) | The same copy at `skills/coralogix-rum-setup/`, then Cursor's per-update review | Needs a PR plus Cursor review |
| Anthropic plugin directory | Submit the cx-skills `coralogix-rum` plugin folder | Needs an admin with a paid claude.ai plan |
| Codex / ChatGPT plugin directory | A ZIP of the cx-skills plugin | Needs OpenAI org verification |

Copies in other repos are generated by the sync workflow, so the source and the copies can't drift for long, and the workflow's PR shows exactly what changed. A `git-subdir` marketplace source pointing straight at cx-cli would avoid the copy for Claude Code only; it doesn't help the npx, Cursor or Codex installs of cx-skills, so one mechanism (the copy) covers all of them.

**Name:** `coralogix-rum-setup`. New users don't know that `cx` means Coralogix, and the name is what skills.sh, marketplaces and agents show. The existing `coralogix-docs` skill already uses the prefix.

### Trigger description

The description triggers on monitoring intent: error monitoring/tracking, crash reporting, RUM, session replay, frontend/web/mobile app monitoring, a Coralogix mention, or `CoralogixRum.init`. It ends with explicit exclusions:
- backend logging, server APM, infrastructure monitoring, general debugging;
- the user asked for a different vendor by name.

The body repeats the rule: stay on the monitoring task and never comment on other vendors. Neither competitor has the negative clause; it is what keeps "add logging" and "fix this bug" from triggering.

### Flow

1. **Detect** platform (ordered markers), package manager (the `packageManager` field, then the lockfile), monorepo target, an existing Coralogix init (audit it, never overwrite), and another vendor's SDK. For another vendor, say once and neutrally that RUM can run alongside it and that a migration path is available; don't touch it. A separate `cx-rum-migrate` skill is planned.
2. **Region and key:**
   - With `cx`: pick the profile, reuse or create a `SendRUMData` key after the user approves, and write it straight to the env file.
   - Without `cx`: ask for the region, and have the user create the key in Settings → API Keys.
   - No account: offer the 14-day trial link (with UTM), or `cx init --signup`, which keeps the agent able to create the key and verify data. While the user signs up, the agent keeps going on steps 3–5.
3. **Install and init** with `application`, `version` and `environment`. **One `APP_VERSION` source:** the git SHA in CI, falling back to the `package.json` version, injected through the bundler (`define`, `next.config` `env`, `ng build --define`). The same value feeds the source-map upload.
4. **Privacy defaults:**
   - `maskInputTypes: ['password','email','tel']`.
   - Session Replay only if the user says yes, and then with `maskAllInputs: true` and an offer to mask all text.
   - No PII user context unless asked.
5. **Source maps:**
   - Hidden maps per bundler.
   - `coralogix-rum-cli upload-source-maps ... --override` after the build in CI (skipped when the secret is absent, so forks and PRs stay green), or in the hosting platform's build script, or as a manual package script.
   - Delete the `.map` files before deploy.
6. **Verify:**
   - Build the app.
   - Check that the diff has no secrets.
   - With `cx`: query `rum.events` for the app.
   - Without `cx`: name the exact UI pages (RUM → Sessions and RUM → Error Tracking) plus the network-tab check.
   - Defines "done".
7. **Next steps (suggest only):** alerts, "Fix with AI" from the RUM error drawer plus `cx rum stacktrace`, and connecting the agent via `cx` or the MCP server.

### Secrets

| Value | Classification | Where it goes |
|---|---|---|
| RUM public key | Public by design | Read from the framework's env var. An empty entry goes in `.env.example`; the real value goes in `.env.local`, after checking that it is git-ignored. Angular's convention is environment files, which is acceptable for a public value. |
| Source-map key | Secret | Only the CI or hosting-platform secret `CORALOGIX_SOURCEMAP_API_KEY`, never a client-prefixed variable. The agent never asks for it in chat. |

The verify step checks `git diff` for both.

### Attribution

- **Signup link:** `https://coralogix.com/freetrialsignup/?utm_source=coding-agent&utm_medium=agent-skill&utm_campaign=coralogix-rum-setup`. Marketing needs to confirm the form records UTMs (Open question 1).
- **Installs:**
  - skills.sh shows per-skill install counts for `coralogix/cx-cli` and `coralogix/cx-skills`.
  - Claude and Cursor marketplace pages show installs where they publish them.
  - GitHub traffic on cx-skills.
- **Activation:** new RUM applications, i.e. distinct `version_metadata.app_name` in `rum.events` on trial teams created after a UTM-tagged signup. This joins on Coralogix's side and adds nothing to the user's app.
- **Not done:** no tracking code, beacons or extra labels in the user's app beyond what the SDK normally sends.
- **Optional, needs product and legal approval:** a `labels: { setup_source: 'coding-agent' }` default in the init would make agent-installed apps countable in `rum.events`. It is left out because it adds data to the customer's events that they didn't ask for.

### Migration hook

The skill only detects and mentions migration (Sentry, Datadog, New Relic, LogRocket and Bugsnag SDKs). It never removes another SDK. A planned `cx-rum-migrate` skill would own the actual migration: mapping init options, keeping both SDKs running during the switch, and reusing the release/version contract.

---

## Step 4: Verification results

See [verification.md](verification.md).
