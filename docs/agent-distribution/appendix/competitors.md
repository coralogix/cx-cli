# Competitor teardown: "set up RUM / SDK" coding-agent skills (Sentry, Datadog)

Research date: 2026-11-05. Read-only. Repos were shallow-cloned and only read.

- Sentry `getsentry/sentry-for-ai` @ `8b49e14` (commit date 2026-11-03)
- Datadog `datadog-labs/agent-skills` @ `6bd95cc` (commit date 2026-11-03)

All links below are pinned to those commits unless noted. Quotes are kept under 15 words; everything else is paraphrased. Items marked **OPEN** are unconfirmed.

---

## 1. Sentry

### 1.1 Repo layout and distribution

- `sentry-for-ai` is a **source repo, not an installable plugin**. Per-agent plugins are built by `src/plugins/<agent>/build.sh` and pushed to generated distribution repos (`getsentry/plugin-claude`, `plugin-cursor`, `plugin-codex`, `plugin-copilot`, `plugin-grok`, `plugin-antigravity`, plus `getsentry/agent-plugin` for the Agent Plugins 1.0.0 standard). The skills are also served over HTTP at skills.sentry.dev and through skills.sh. [README][s-readme]
- Layout: `src/skills/<skill>/SKILL.md` plus an optional `references.yml`. Shared docs live in `src/references/` (`concepts/`, `debug-artifacts/`, `releases/`, `semantics/`, and top-level files). Per-agent manifests and build scripts live in `src/plugins/<agent>/`, the version in `src/plugins/version.json`, and the generated index in `src/SKILL_TREE.md`. [AGENTS.md][s-agents]
- **Build-time hydration**: each skill's `references.yml` lists the shared files it needs (globs allowed), and the build copies them into the skill so every shipped skill is self-contained. [AGENTS.md][s-agents], [sentry-instrument/references.yml][s-instr-refs]
- Rule for where content goes: content read by two or more skills goes in the shared library; content used by one skill goes in that skill's own `references/`. Group folders must not link across domains. [AGENTS.md][s-agents]
- Manifests found:
  - Claude: `plugin.json` declares the MCP server inline (`mcpServers`, `https://mcp.sentry.dev/mcp?utm_source=plugin`), and `marketplace.json` uses `source: "./"`. [claude/plugin.json][s-claude-plugin], [claude/marketplace.json][s-claude-mkt]
  - Cursor: `plugin.json` (with a logo) plus a root `mcp.json` consumed as-is. [cursor/plugin.json][s-cursor-plugin], [mcp.json][s-mcp]
  - Codex: `plugin.json` with an `interface` block (displayName, `defaultPrompt` examples, brandColor, privacy/ToS URLs, `capabilities: ["Read","Write"]`). The MCP config ships as `.mcp.json`. A `hide-skills.py` transform maps `disable-model-invocation` to Codex's `agents/openai.yaml` `allow_implicit_invocation: false`. [codex/plugin.json][s-codex-plugin], [hide-skills.py][s-codex-hide]
  - Agent Plugins standard: `plugin.json` and `mcp.json` with agent-plugins.org schemas (`streamable-http`). [agent-plugin/plugin.json][s-ap-plugin], [agent-plugin/mcp.json][s-ap-mcp]
- All agent plugins version together from one `version.json`. Each distribution repo has a `develop` branch (every push to main) and a `main` branch (releases only, tagged). Anthropic's marketplace pins `plugin-claude` by SHA. [AGENTS.md][s-agents]
- License: the repo and installer are **MIT** ([LICENSE][s-license], [installer package.json][s-inst-pkg]), but every skill's frontmatter says `license: Apache-2.0` ([sentry-instrument SKILL.md][s-instr]). This is an inconsistency.

### 1.2 Installer (`npx @sentry/agent-plugin`)

- npm: `@sentry/agent-plugin` 0.4.1, MIT, last modified 2026-09-16 (from `npm view @sentry/agent-plugin`). Requires Node >= 22.13. [installer README][s-inst-readme]
- It detects installed assistants, lets the user choose, and runs each tool's **native** plugin command [installer README][s-inst-readme]:
  - Claude Code: `claude plugin install sentry` (official marketplace)
  - Codex: `codex plugin add sentry`
  - Cursor: clones `getsentry/plugin-cursor` into `~/.cursor/plugins/local/sentry`
  - Grok: `grok plugin install getsentry/plugin-grok`
- After installing, it offers to **copy a starter prompt** to the clipboard (default: the get-started prompt). `install "Setup logging"` uses a custom prompt instead. `install org-slug#code` builds an onboarding prompt that carries a 10-character run code from Sentry's web onboarding page. `--no-interactive` installs to every detected agent. `remove`/`uninstall` is also supported. [installer README][s-inst-readme]
- Docs page: one command for all assistants, with `dotagents` as an alternative. It does not list skills and does not mention auth method, signup, privacy, or telemetry. [docs.sentry.io/ai/agent-plugin][s-docs]

### 1.3 The setup skills (the RUM-equivalent path)

Sentry **retired per-SDK skills** (`sentry-browser-sdk`, `sentry-nextjs-sdk`, `sentry-react-sdk`, `sentry-react-native-sdk`, ...) and froze them under `skills-legacy/`, outside the build. They replaced them with two task-shaped skills that **fetch the docs at runtime**. [README][s-readme], [AGENTS.md][s-agents]

| Skill | Lines | Role |
|---|---|---|
| `sentry-get-started` | 239 | Entry point: intro, a 3-probe check, then routing. For a new project it runs first-error setup itself. [SKILL.md][s-getstarted] |
| `sentry-instrument` | 232 | Orchestrator for "install SDK" and "add a signal" (replay, logs, profiling, ...). [SKILL.md][s-instr] |
| `sentry-fix-stack-traces` | 92 | Source maps and debug files. [SKILL_TREE / file list][s-readme] |

Shared references these skills load (line counts): `first-error-setup.md` 164, `setup-verification.md` 118, `sdk-docs.md` 81, `new-project.md` 52, `auth-token.md` 71, plus `concepts/*`, `debug-artifacts/*`, and `releases/*`.

**Frontmatter / trigger wording.** Only `name`, `description`, and `license` are allowed. Other router fields (`category`, `parent`, `role`, `disable-model-invocation`) are **rejected by lint**, and every skill can be invoked by the model. [AGENTS.md][s-agents]
- `sentry-get-started`: "Guided entry point for using Sentry through your agent." [SKILL.md][s-getstarted]
- `sentry-instrument`: "Use to add Sentry to a project or to capture more than errors." [SKILL.md][s-instr]
- The legacy browser skill used quoted user phrases, for example "add Sentry to a website" and "install @sentry/browser". It was marked `disable-model-invocation: true` behind a router. [legacy browser SKILL.md][s-legacy-browser]
- None of these descriptions has a negative ("do not use when...") clause.

**Structure of `sentry-instrument`.** It has 5 steps: Set scope → Detect ownership and install → Wire signals → Verify → Suggest next.
- The scope table has three modes: *First error* (the default when unsure), *Add a signal*, and *Full setup*.
- It explicitly says not to over-instrument: replay, logging, profiling, and similar signals are deferred unless the user asks.
- The SKILL.md is described as "the orchestration", and the agent should not read a reference before it needs it. [SKILL.md][s-instr]

**Framework / package-manager detection.**
- `sdk-docs.md` maps marker files and dependencies to a docs URL. Examples: `next` → Next.js, `react-native`/`expo` → React Native, `Package.swift`/`Podfile` → Apple, `build.gradle` → Android, `pubspec.yaml` with `flutter` → Flutter.
- The rule is to prefer the **most specific framework** (Next.js over React over Node).
- The agent must **tell the user what it found and confirm**, not assume from files alone. [sdk-docs.md][s-sdkdocs]
- The current skills have **no explicit package-manager detection**; they defer to the docs' install command. [sdk-docs.md][s-sdkdocs] (Inference from grep: only Bun is detected, via `bun.lock`.)
- The legacy per-SDK skills had their own "Phase 1: Detect" sections. [legacy browser SKILL.md][s-legacy-browser]

**Docs sync = fetched at runtime.** The skill fetches per-platform install, `init`, and signal code from docs.sentry.io; any docs page is available as Markdown by appending `.md`. Because missing pages return HTTP 200 with a "Page Not Found" title, the skill checks the title rather than the status. [sdk-docs.md][s-sdkdocs]
- A weekly "Skill Drift Detector" workflow runs one LLM session per SDK. Each compares released SDK PRs against `src/references/sdks/<sdk>/` and opens a PR or issue. The agent job has read-only permissions, and a deterministic job applies changes inside a path allowlist. [skill-drift.yml][s-drift]
- **OPEN**: `src/references/sdks/` does not exist at this commit (the content moved to runtime docs fetch), so this workflow may now be stale.
- PR-label-triggered evals run against an internal `sentry-for-ai-evals` repo across harness/model targets, comparing merge-base against head. [ai-evals.yml][s-evals]

**Accounts / DSN / auth.**
- The plugin bundles the **Sentry MCP** server. The skills require it for provisioning and verification. If it is not authenticated, the agent should suggest the harness-appropriate way to authenticate it. [sentry-instrument][s-instr], [README][s-readme]
- No account: get-started asks via an interactive prompt (`AskUserQuestion`), then sends the user to the signup page. There is no agent signup flow. Example wording: "point them to https://sentry.io/signup". It also covers "MCP not installed" (link to mcp.sentry.dev) and "has an account" (connect, then rerun the probe). [sentry-get-started][s-getstarted], [new-project.md][s-newproj]
- The signup link has **no UTM parameter** ([sentry-get-started][s-getstarted]).
- DSN: found via MCP with `find_organizations`/`find_projects`/`find_teams`, then `find_dsns`. Otherwise the agent **proposes `create_project` and creates it only on a yes**, "never silently". Handled edge cases: a DSN returned as `unavailable` (then call `create_dsn`), and a member 403 (the user creates the project in the UI). [new-project.md][s-newproj]
- Build-time auth token: kept separate from the DSN. The agent **checks only that it is present** and never reads or echoes the value; there is a bash snippet that prints set/unset. Org tokens are preferred over personal tokens. A committed token means revoke it, not delete the line. The doc includes a failure-signature table (silent skip, 401, 403, works locally but not in CI). [auth-token.md][s-authtoken]
- Wizard: the current skills mention `npx @sentry/wizard@latest -i sourcemaps` only as "Path C". It is interactive with browser login, "so the **user** runs it, not the agent". [debug-artifacts/javascript.md][s-js-sm] The legacy Next.js skill told the user to run `npx @sentry/wizard@latest -i nextjs` themselves. [legacy nextjs SKILL.md][s-legacy-next]

**Source maps.** [debug-artifacts/javascript.md][s-js-sm]
- Two things must both be true: the build emits maps, and the maps reach Sentry tied to the built files through **Debug IDs**.
- Prefer **hidden** source maps for browsers.
- Path A is the bundler plugin (Vite/webpack/Rollup/esbuild/`withSentryConfig`). It reads `SENTRY_ORG`/`SENTRY_PROJECT`/`SENTRY_AUTH_TOKEN` from the environment and should delete maps after upload.
- Path B is `sentry-cli sourcemaps inject` then `upload`.
- Path C is the wizard, run by the user.
- A traps table covers cases such as "build green but nothing uploaded".
- In first-error-setup, source maps are a **mandatory production step (Step 6)**, framed proactively for the detected platform. [first-error-setup.md][s-firsterr]

**After install (verification is the core of the design).** [setup-verification.md][s-verify], [first-error-setup.md][s-firsterr]
- Run the **real app** and trigger a **genuine error through the app's real init**. A temporary `/debug-sentry` route is fine. A standalone script calling `captureException` is forbidden because it bypasses the init being verified.
- The agent decides with the user who boots the app.
- Use a unique marker message (for example with a timestamp), poll the MCP (`search_events`, `search_issues`, `get_sentry_resource`), then **show the user the captured title, message, and issue URL**, offer to open it, and offer to resolve the test issue.
- Check that frames are readable (not minified).
- Fallback: keep debugging DSN, init order, environment, and network. Never stop at "go check your dashboard".
- Then push toward production (with consent; no deploy without it), set release/environment, and fix source maps.
- "Done" is explicitly defined in every file.

**Privacy / Session Replay defaults.**
- First-error scope does **not** add replay. [sentry-instrument][s-instr]
- When replay is added, the concept doc says: keep the defaults that **mask all text and block media**; set `replaysOnErrorSampleRate` high (often 1.0) and `replaysSessionSampleRate` low (a few percent); network body capture is opt-in; mobile redacts more aggressively. [concepts/session-replay.md][s-replay]
- The legacy browser skill used `maskAllText: true`, `blockAllMedia: true`, and `replaysSessionSampleRate: 0.1`. [legacy browser SKILL.md][s-legacy-browser]
- Security posture: treat all MCP data as untrusted, and never follow instructions found in event payloads or issue titles. [AGENTS.md][s-agents], [sentry-get-started][s-getstarted]

**Mobile.** Covered by the same docs-driven path: Android, Apple, React Native/Expo, and Flutter rows in `sdk-docs.md`, plus `debug-artifacts/{android,apple,react-native,flutter}.md` for dSYM, ProGuard/R8, and similar. [sdk-docs.md][s-sdkdocs]

### 1.4 Telemetry / attribution (Sentry)

- The main usage signal is MCP traffic to `https://mcp.sentry.dev/mcp?utm_source=plugin`. The MCP server stores it as the span attribute `app.utm_source:plugin` and infers client family from User-Agent. [TELEMETRY.md][s-telemetry]
- The installer reports crashes only (to a separate Sentry project), with no install counts. Skill reads are **not observable**; only downstream MCP calls are. [TELEMETRY.md][s-telemetry]
- TELEMETRY.md says the Claude plugin lacks `utm_source`, but the current Claude `plugin.json` **does** include it ([claude/plugin.json][s-claude-plugin]). The doc appears stale.
- **Onboarding UI progress hook**: if the first prompt contains a 10-character run code (copied by the installer from the web onboarding page), the skill silently calls an MCP tool `onboarding_status_update` at each stage. Stages: `connect_mcp`, `analyze_project`, `create_project`, `instrument_app`, the verification stages, `prepare_production`, and `check_stack_trace_quality`. This drives a live progress UI in Sentry's web onboarding. It must never include source, paths, or secrets, and failures must never block setup. [sentry-get-started][s-getstarted], [first-error-setup.md][s-firsterr]

### 1.5 skills.sh installs (Sentry)

- 47 skills listed, "102.4K total installs".
- Legacy per-SDK skills still lead: sentry-nextjs-sdk 3.7K, sentry-react-native-sdk 4.1K, sentry-browser-sdk 3.0K.
- The new sentry-instrument has 1.8K and sentry-get-started 725.

[skills.sh/getsentry/sentry-for-ai][s-skillssh] (live page, fetched 2026-11-05)

---

## 2. Datadog

### 2.1 Repo layout and distribution

- Flat repo with one directory per skill. Nested sub-skills sit under a parent, for example `dd-browser-sdk/{setup-sourcemaps,upgrade-v5,upgrade-v6,upgrade-v7}/SKILL.md`. [README][d-readme]
- `.claude-plugin/plugin.json` lists every skill path explicitly. `.claude-plugin/marketplace.json` makes the repo its own marketplace (`source: "./"`). The README says Copilot CLI and Claude Code install via `plugin marketplace add datadog-labs/agent-skills`. [plugin.json][d-plugin], [marketplace.json][d-mkt], [README][d-readme]
- No Cursor- or Codex-specific manifests and no AGENTS.md at this commit (observed from the file list). Other agents use `npx skills add datadog-labs/agent-skills --skill <name> --full-depth -y` (vercel-labs `skills` installer) or `cp -r` into `~/.claude/skills`. [README][d-readme]
- Rust crate: `Cargo.toml` and `src/lib.rs` embed selected SKILL.md files with `include_str!` (for example `DD_BROWSER_SDK_SKILL` and `upgrade-v7`). [lib.rs][d-lib]
  - **OPEN**: who consumes this crate. The `pup` CLI (Rust) has its own `pup skills install` with separately vendored `include_str!` copies, and its Cargo.toml does not reference `agent-skills` ([DataDog/pup src/skills.rs][d-pup-skills]).
- License: **MIT**. [LICENSE][d-license]
- Frontmatter uses `name`, `description`, plus `metadata` (`version`, `author`, `repository`, `tags`, Cursor-style `globs`, `alwaysApply: "false"`). [dd-browser-sdk SKILL.md][d-bsdk], [dd-instrument-rum SKILL.md][d-rum]

### 2.2 The RUM skills (two of them, overlapping)

| Skill | Lines | Role |
|---|---|---|
| `dd-browser-sdk` | 44 | **Router only**: upgrades v4→5, v5→6, v6→7, and sourcemaps. [SKILL.md][d-bsdk] |
| `upgrade-browser-sdk-v7` (`dd-browser-sdk/upgrade-v7`) | 245 | v6→v7 migration in 6 steps, each with a grep pattern. [SKILL.md][d-v7] |
| `setup-browser-sdk-sourcemaps` | 384 | Datadog build-plugin source-map upload. [SKILL.md][d-sm] |
| `dd-instrument-rum` | 111 (+7 refs, ~436 lines) | **The actual install skill.** [SKILL.md][d-rum] |

- **Trigger wording.**
  - `dd-browser-sdk`: "Use when upgrading Browser SDK versions, setting up RUM or Logs". It also carries `globs` on `@datadog/browser-*`. However, it routes only to upgrades and source maps, not to an install. [SKILL.md][d-bsdk]
  - `dd-instrument-rum`: "Use when asked to add, set up, instrument, repair, or verify Datadog RUM". [SKILL.md][d-rum]
  - Both claim "set up RUM". On skills.sh, dd-browser-sdk has 625 installs against dd-instrument-rum's 64. [skills.sh][d-skillssh]
- The router uses **hard-coded `.claude/skills/dd-browser-sdk/...` paths** with an instruction to read them immediately. [SKILL.md][d-bsdk] (Inference: this is Claude-path-specific and may not resolve under plugin or other-agent installs.)
- **Scope guard (in the body, not the description).** Instrument only the browser app. It explicitly lists what not to add: APM, tracing, Logs, source-map upload, user identification, and others. It also forbids adding `allowedTracingUrls`. [dd-instrument-rum][d-rum], [rum-core.md][d-rumcore]

**Detection (more explicit than Sentry).** [dd-instrument-rum][d-rum]
- Monorepo: find the frontend manifest used by the **real build** (from workspace config, Dockerfiles, CI). Ask if several apps qualify. Reject non-browser apps.
- Framework precedence: nuxt → next (App vs Pages Router) → @angular/core (standalone vs NgModule) → vue → svelte/kit → react → vanilla.
- Also detects router versions, TypeScript, and Node version.
- Package manager: the `packageManager` field first, then the lockfile (bun/pnpm/yarn/npm). Never mix managers.
- Bundler: Vite/webpack/Rollup/esbuild/Rspack/CRA.
- Entrypoint: from the build config and import graph. Also the real build command and existing public env-var conventions.
- Existing-init detection: searches for aliases, namespace imports, wrapper modules, CDN `DD_RUM.init`, and loader URLs. Package presence alone is "insufficient".
- Decision table:
  - No init → add one.
  - One valid init → **treat every existing value as immutable** and only add plugin wiring.
  - Invalid init → stop.
  - Multiple or mixed npm/CDN → stop and report.
  - Complete → no-op, but still verify.
- Framework references: `rum-react` (React 18/19; React Router v6/v7/v8 entrypoints; TanStack Router ≥1.64), `rum-nextjs` (requires Next ≥15.3; uses `instrumentation-client.ts`), angular, vue, nuxt, and other-frameworks. Version prerequisites are validated **before** editing; never upgrade the framework or silently fall back. [rum-react.md][d-react], [rum-nextjs.md][d-next]
- It posts a short checklist before changes and updates it as it goes. [dd-instrument-rum][d-rum]

**Credentials (applicationId / clientToken / site).** [common-credentials.md][d-creds]
- Reuse supplied, environment, or existing values. The two IDs are "browser-visible identifiers, not Datadog API keys". The skill never asks for an API key.
- The `site` must be in an allowlist of 9 public sites. If it is invalid, stop and ask; do not silently default to US1.
- If nothing is supplied, ask whether a RUM app already exists. If not, call a **`CreateRumApplication(name, type)`** tool "when it is available". **OPEN**: this tool is not defined anywhere in the repo, and Datadog MCP/pup provenance is unconfirmed. It may be a tool in Datadog's own agent harness.
- Fallback: stop without editing and send the user to `app.datadoghq.com/rum/application/create` (no UTM).
- Env vars: follow the existing convention (`VITE_`, `NEXT_PUBLIC_`, `REACT_APP_`, `VUE_APP_`). Without a convention, inline the values, because there is no need to "hide" public IDs. Never write IDs or paths to persistent memory. [dd-instrument-rum][d-rum]
- Account creation lives in a separate skill, `dd-account-setup` (215 lines + references). It detects keys, then **asks** the user to choose one of three paths:
  - A: use an existing key.
  - B: OAuth PKCE in the browser, using a local `python3` callback listener.
  - C: **automated in-terminal account signup** with a generated password written to `.env`. The user verifies an emailed 8-digit code. The agent must mention the MSA, Privacy, and Cookie policies before submitting.

  [dd-account-setup][d-acct], [authenticate.md][d-auth]

**Defaults (canonical init).** [rum-core.md][d-rumcore]
- `sessionSampleRate: 100`, `sessionReplaySampleRate: 20`, `defaultPrivacyLevel: 'mask-user-input'`, plus `trackUserInteractions`, `trackResources`, and `trackLongTasks` all true.
- `service`, `env`, and `version` come from package metadata or deploy config. Never invent them; use an obvious placeholder only in non-interactive mode, and report it.
- npm is preferred. CDN is used only for unmanaged HTML, with an async loader and a per-site v7 URL table.
- All `@datadog/browser-rum*` packages are pinned to the **same exact version**.

**Source maps (`setup-browser-sdk-sourcemaps`).** [SKILL.md][d-sm]
- The agent must create 7 todos first.
- Uses the Datadog build plugin (`@datadog/{vite,webpack,rollup,rspack,esbuild}-plugin`) with supported-version tables.
- **Next.js/Turbopack are out of scope**: use `datadog-ci sourcemaps upload` in `postbuild` instead.
- Three values must match the RUM init or the real deploy: `service`, `releaseVersion`, `minifiedPathPrefix`. The prefix is derived from bundler `base`, CI/deploy config, and rewrite rules; otherwise ask, with devtools instructions.
- The skill never writes placeholders. It refuses if there is no `datadogRum.init`.
- `DATADOG_API_KEY` must be supplied as a CI secret, and the skill maps each CI provider to where that secret goes.
- **Safe verification**: run the build with **no API key reachable**. "No sourcemaps to upload" means maps are not emitted; "No authentication token provided" means everything is wired correctly. The skill explicitly distrusts `dryRun`. A real upload happens only with explicit user confirmation of which key and org will be used.
- It includes a zsh-safe `find` tip.

**Verification of the install.** [common-verify-report.md][d-verify]
- Confirm exactly one init and that packages are persisted in the manifest and lockfile (a `node_modules` hit is not proof). Run the formatter on changed files and the **project's own terminating build**.
- Optionally run a bounded local browser session to look for a RUM intake request.
- "Do not claim that sessions are visible in Datadog" based on a build alone.
- There is **no backend query** to confirm data landed (unlike Sentry).
- The output is a JSON report `{success, rumApplicationId, message}`. This suggests the skill is also run programmatically, but that is **OPEN** and unconfirmed.

**Mobile.** Only `dd-android-sdk` (a router) and `upgrade-v3` (an Android v2→v3 migration) exist. There is **no** setup skill for iOS, Android install, React Native, or Flutter. [dd-android-sdk][d-android]

### 2.3 Docs sync / runtime fetch (Datadog)

- RUM install content is **bundled** and static, for example hard-coded v7 CDN URLs. [rum-core.md][d-rumcore]
- `dd-docs` prefers the MCP `search_datadog_documentation` tool, then one page from `docs.datadoghq.com/llms.txt`, and cites the URL. [dd-docs][d-docs]
- The `dd-orchestrator` (beta) composes a plan from `catalog.json` and **fetches skills at runtime** from GitHub `main` or `api.datadoghq.com/api/v2/onboarding/skills/<id>?format=md` ("nothing is pinned"). Its fetcher allowlists hosts, fails closed, and sends `User-Agent: dd-orchestrator-fetch`. [dd-orchestrator][d-orch], [fetch_skill.py][d-fetch]
- The README notes the `datadog-app` skill fetches guidance at run time, "so it does not need reinstalling". [README][d-readme]

### 2.4 Telemetry / attribution (Datadog)

- Signup sends `metadata.referrer:"skill"` and `signup_source:"skill"` in the signup and verify calls. [authenticate.md][d-auth]
- Orchestrator fetches use a distinctive User-Agent. [fetch_skill.py][d-fetch]
- No `utm_` parameters were found anywhere in the repo (grep across `*.md`). The MCP URLs in the README carry `toolsets=` but no UTM. [README][d-readme]

### 2.5 skills.sh installs (Datadog)

"34.5K total installs" across 78 skills: dd-browser-sdk 625, upgrade-browser-sdk-v7 492, upgrade-browser-sdk-v6 78, upgrade-browser-sdk-v5 74, dd-instrument-rum 64, setup-browser-sdk-sourcemaps 56, dd-account-setup 61, dd-orchestrator 39, dd-android-sdk 22. [skills.sh/datadog-labs/agent-skills][d-skillssh] (live page, fetched 2026-11-05)

---

## 3. Cross-cutting

- **Not triggering on unrelated prompts.**
  - Neither vendor puts negative clauses in `description`.
  - Sentry relies on task-shaped descriptions and a single entry point. [AGENTS.md][s-agents]
  - Datadog uses Cursor-style `globs` and `alwaysApply: "false"` metadata, plus in-body scope exclusions. [dd-browser-sdk][d-bsdk], [dd-instrument-rum][d-rum]
  - The orchestrator description restricts itself to goals stated "without naming a specific product or skill". [dd-orchestrator][d-orch]
- **Vendor neutrality / coexistence.**
  - Neither skill mentions competitor SDKs already in the repo (grep for other vendor names returned nothing relevant).
  - Sentry does have a single-owner rule: never a second Sentry init, OTLP exporter, or AI span producer, and existing OpenTelemetry/framework instrumentation is preserved. [sentry-instrument][s-instr]
  - Datadog's duplicate-init logic covers only Datadog RUM. [dd-instrument-rum][d-rum]
- **Telemetry and attribution**:
  - Sentry: a UTM on the MCP URL plus the onboarding run-code progress tool. [TELEMETRY.md][s-telemetry], [sentry-get-started][s-getstarted]
  - Datadog: `signup_source: skill` and a fetch User-Agent. [authenticate.md][d-auth]
  - Neither puts a UTM on the human-facing signup links.
- **Docs sync**:
  - Sentry: runtime fetch of docs.sentry.io `.md` pages, plus an LLM drift job and PR evals. [sdk-docs.md][s-sdkdocs], [skill-drift.yml][s-drift], [ai-evals.yml][s-evals]
  - Datadog: bundled references for RUM, with runtime fetch only in the orchestrator and docs skills. [rum-core.md][d-rumcore], [fetch_skill.py][d-fetch]
- **Secrets hygiene (both)**:
  - Public IDs vs. build secrets are explicitly distinguished.
  - Secrets go in CI secret stores and are never echoed or committed.
  - Presence is checked without revealing the value.

  [auth-token.md][s-authtoken], [common-credentials.md][d-creds], [setup-browser-sdk-sourcemaps][d-sm]

---

## 4. Patterns worth copying

1. **A thin orchestrating SKILL.md plus lazily loaded references**, with "don't read a reference before you need it" and an explicit "What done looks like" section. Both vendors converge on this. [s-instr][s-instr], [d-rum][d-rum]
2. **Scope modes with a conservative default.** First-error / add-a-signal / full-setup, defaulting to minimal. Do not enable replay or extra signals unasked; propose them afterward. [s-instr][s-instr]
3. **Deterministic detection with precedence** (meta-framework beats UI library), real-build manifest discovery in monorepos, and package manager from `packageManager` then the lockfile, never mixing managers. Then **confirm detection with the user**. [d-rum][d-rum], [s-sdkdocs][s-sdkdocs]
4. **Existing-init guardrail**: treat a valid existing init as immutable, never create a second init, stop on conflicting or mixed CDN/npm setups, and diff to prove pre-existing values are byte-identical. [d-rum][d-rum], [d-rumcore][d-rumcore]
5. **Prove it end to end against the backend.** Run the real app, trigger a uniquely tagged event through the real init (never a standalone script), poll the API/MCP, and show the user the captured event and a deep link. Never end with "check your dashboard". [s-verify][s-verify] Combine this with Datadog's "build must pass; never claim telemetry you didn't observe". [d-verify][d-verify]
6. **Credential provisioning via tool, with consent.** Find an existing app/project through API/MCP. Create one only on an explicit yes. Fall back to a deep link in the UI. Never fabricate IDs. Validate `site`/region against an allowlist instead of silently defaulting. [s-newproj][s-newproj], [d-creds][d-creds]
7. **Public-ID vs. secret separation**: inline public RUM IDs following the project's env-prefix convention, keep upload keys in CI secrets, and check presence without echoing. Include a symptom→cause failure table. [s-authtoken][s-authtoken], [d-creds][d-creds]
8. **Source maps as a first-class production step**, including Datadog's credential-less safe-verification trick ("no key reachable" proves maps were found without uploading) and refusing to guess `service`/`version`/path prefix. [d-sm][d-sm], [s-js-sm][s-js-sm]
9. **One source tree, build per agent**: per-agent manifests, a lockstep version, a shared reference library hydrated at build time, plus a one-command installer that calls each agent's native plugin command and copies a starter prompt. [s-agents][s-agents], [s-inst-readme][s-inst-readme]
10. **Attribution that respects privacy**: `utm_source` on the bundled MCP URL, `signup_source` on API-driven signup, and an optional web-onboarding run code that streams privacy-safe stage updates. [s-telemetry][s-telemetry], [d-auth][d-auth], [s-getstarted][s-getstarted]

## 5. Gaps / things they do poorly

1. **Overlapping triggers (Datadog).** `dd-browser-sdk` claims "setting up RUM" but only routes to upgrades and source maps; `dd-instrument-rum` does the install. Installs are 625 vs. 64, so most users likely get the wrong skill for setup. Its router also hard-codes `.claude/skills/...` paths. [d-bsdk][d-bsdk], [d-skillssh][d-skillssh]
2. **Mobile is thin or absent at Datadog.** There is only an Android v2→v3 upgrade; no iOS, React Native, Flutter, or Android install skill. [d-android][d-android] Sentry covers mobile only through runtime docs, without curated mobile RUM guidance (privacy masking on mobile is a single line). [s-replay][s-replay]
3. **No verification against the backend at Datadog.** Success equals a passing build; there is no query to confirm sessions arrived. [d-verify][d-verify]
4. **Stale or inconsistent metadata at Sentry.** TELEMETRY.md contradicts the Claude manifest on `utm_source`, the drift workflow targets a removed `src/references/sdks/` directory (**OPEN**), and the license is MIT for the repo but Apache-2.0 in skill frontmatter. [s-telemetry][s-telemetry], [s-claude-plugin][s-claude-plugin], [s-drift][s-drift], [s-license][s-license]
5. **No "already using another vendor" handling** in either skill, and no negative trigger clauses. Neither has UTM on human-facing signup links, so they cannot attribute skill-driven signups that go through the browser. [s-getstarted][s-getstarted], [d-creds][d-creds]

---

## Open questions

- Where does `CreateRumApplication` come from (Datadog MCP? a Datadog-hosted agent?), and is the `dd-instrument-rum` JSON report consumed programmatically? [d-creds][d-creds], [d-verify][d-verify]
- Who consumes the `agent-skills` Rust crate (`src/lib.rs`)? It is not `pup` by Cargo dependency. [d-lib][d-lib], [d-pup-skills][d-pup-skills]
- Is Sentry's skill-drift workflow still effective after the move to runtime docs? [s-drift][s-drift]
- Does the Sentry MCP OAuth flow tag signups or first connections with attribution beyond `utm_source=plugin`? This was not visible in this repo.

<!-- link definitions -->
[s-readme]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/README.md
[s-agents]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/AGENTS.md
[s-license]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/LICENSE
[s-telemetry]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/TELEMETRY.md
[s-mcp]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/mcp.json
[s-claude-plugin]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/plugins/claude/plugin.json
[s-claude-mkt]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/plugins/claude/marketplace.json
[s-cursor-plugin]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/plugins/cursor/plugin.json
[s-codex-plugin]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/plugins/codex/plugin.json
[s-codex-hide]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/plugins/codex/hide-skills.py
[s-ap-plugin]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/plugins/agent-plugin/plugin.json
[s-ap-mcp]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/plugins/agent-plugin/mcp.json
[s-inst-readme]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/packages/installer/README.md
[s-inst-pkg]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/packages/installer/package.json
[s-getstarted]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/skills/sentry-get-started/SKILL.md
[s-instr]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/skills/sentry-instrument/SKILL.md
[s-instr-refs]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/skills/sentry-instrument/references.yml
[s-sdkdocs]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/references/sdk-docs.md
[s-firsterr]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/references/first-error-setup.md
[s-newproj]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/references/new-project.md
[s-authtoken]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/references/auth-token.md
[s-verify]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/references/setup-verification.md
[s-js-sm]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/references/debug-artifacts/javascript.md
[s-replay]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/src/references/concepts/session-replay.md
[s-legacy-browser]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/skills-legacy/sentry-browser-sdk/SKILL.md
[s-legacy-next]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/skills-legacy/sentry-nextjs-sdk/SKILL.md
[s-drift]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/.github/workflows/skill-drift.yml
[s-evals]: https://github.com/getsentry/sentry-for-ai/blob/8b49e144584cb846ded61a74308372149d4cea40/.github/workflows/ai-evals.yml
[s-docs]: https://docs.sentry.io/ai/agent-plugin
[s-skillssh]: https://skills.sh/getsentry/sentry-for-ai
[d-readme]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/README.md
[d-license]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/LICENSE
[d-plugin]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/.claude-plugin/plugin.json
[d-mkt]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/.claude-plugin/marketplace.json
[d-lib]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/src/lib.rs
[d-bsdk]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-browser-sdk/SKILL.md
[d-v7]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-browser-sdk/upgrade-v7/SKILL.md
[d-sm]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-browser-sdk/setup-sourcemaps/SKILL.md
[d-rum]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-instrument-rum/SKILL.md
[d-rumcore]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-instrument-rum/references/rum-core.md
[d-react]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-instrument-rum/references/rum-react.md
[d-next]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-instrument-rum/references/rum-nextjs.md
[d-creds]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-instrument-rum/references/common-credentials.md
[d-verify]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-instrument-rum/references/common-verify-report.md
[d-acct]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-account-setup/SKILL.md
[d-auth]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-account-setup/references/authenticate.md
[d-android]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-android-sdk/SKILL.md
[d-docs]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-docs/SKILL.md
[d-orch]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-orchestrator/SKILL.md
[d-fetch]: https://github.com/datadog-labs/agent-skills/blob/6bd95cce9e0792c0f27c342214609706e51313c3/dd-orchestrator/scripts/fetch_skill.py
[d-pup-skills]: https://github.com/DataDog/pup/blob/main/src/skills.rs
[d-skillssh]: https://skills.sh/datadog-labs/agent-skills
