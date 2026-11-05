# Distribution channels for a public "set up Coralogix RUM" agent skill

Research date: 2026-11-05. All sources were fetched that day. "Open question" means the item was not confirmed from an official source.

Coralogix already has these listings (found while researching, useful context):

| Asset | Where | Evidence |
|---|---|---|
| `coralogix/cx-cli/skills` (32 skill names, ~64K installs, including stale names such as `cx-query-logs` and `cx-rum`) | skills.sh | https://skills.sh/coralogix/cx-cli |
| `coralogix/cx-skills` (public; already ships `.claude-plugin/`, `.codex-plugin/`, `.cursor-plugin/`, and Tessl) | GitHub, self-hosted marketplace `cx-skills`, plugin `coralogix` | https://github.com/coralogix/cx-skills |
| `coralogix/coralogix-cursor-plugin` (MCP, skills `cx-config` and `cx-setup`, rule `coralogix-mcp`) | Cursor Marketplace as "Coralogix" | https://cursor.com/marketplace/coralogix-ltd |
| Remote MCP server `com.coralogix/mcp-server` v2.0.0 (DNS namespace, status active) | Official MCP Registry | `https://registry.modelcontextprotocol.io/v0/servers?search=coralogix` |
| Coralogix MCP connector, "Anthropic verified", added June 2026 | Anthropic connector directory | https://claude.com/connectors/coralogix |
| Coralogix plugin | `claude-plugins-official` and `claude-community`: **not listed** (no "coralogix" match in either `marketplace.json`) | https://github.com/anthropics/claude-plugins-official, https://github.com/anthropics/claude-plugins-community |

---

## 1. Claude Code plugins and marketplaces

**Mechanism.** A plugin is a directory. Its optional manifest is `.claude-plugin/plugin.json`, and skills go under `skills/<name>/SKILL.md` at the plugin root. A marketplace is a git repo with a `.claude-plugin/marketplace.json` that lists plugins.
- Manifest reference: https://code.claude.com/docs/en/plugins-reference
- Marketplace reference: https://code.claude.com/docs/en/plugins/marketplace-reference.md

**plugin.json.**
- `name` is the only required field. The manifest itself is optional.
- `skills` takes a path or an array of paths and *adds to* the default `skills/` scan.
- "A plugin with `SKILL.md` at its root, no `skills/`, and no `skills` key loads as a single skill."
- Names that start with `claude-`/`anthropic-` are refused.
- Fields used only by the Anthropic directory listing: `icon`, `documentationUrl`, `supportUrl`, `privacyPolicyUrl`, `termsOfServiceUrl`.
- Source: https://code.claude.com/docs/en/plugins-reference

```json
{ "name": "coralogix-rum", "description": "...", "version": "0.1.0",
  "author": { "name": "Coralogix" }, "homepage": "https://...", "repository": "https://github.com/coralogix/cx-cli", "license": "Apache-2.0" }
```

**marketplace.json.**
- Required fields: `name`, `owner.name`, and `plugins[]`. Each plugin entry needs `name` and `source`.
- Reserved names include `claude-plugins-official`, `claude-community`, `agent-skills`, and anything starting with `claude-`.
- Source: https://code.claude.com/docs/en/plugins/marketplace-reference.md

```json
{ "name": "coralogix", "owner": { "name": "Coralogix" },
  "plugins": [ { "name": "coralogix-rum", "source": "./skills/coralogix-rum-setup", "description": "..." } ] }
```

**Can it be a subdirectory? Yes.**
- **Relative path (same repo).** The path starts with `./` and resolves from the marketplace root. `..` is rejected. `"./"` (the repo root) is allowed, which is what Datadog uses. Optional `metadata.pluginRoot` allows bare names.
- **`git-subdir` (another repo).** Example: `{ "source": "git-subdir", "url": "owner/repo", "path": "tools/x", "ref", "sha" }`. It uses a sparse checkout.
- Other source types: `github`, `url`, `npm`, `archive`, and `command`.
- Source: https://code.claude.com/docs/en/plugins/marketplace-reference.md

**Install commands.**
- Shell: `claude plugin marketplace add coralogix/cx-cli`, then `claude plugin install <plugin>@<marketplace-name>`.
- Inside a session: `/plugin marketplace add owner/repo` and `/plugin install <plugin>@<marketplace>`.
- One-step install: `claude plugin install <plugin> --marketplace owner/repo` (needs v2.1.292+ in the shell, v2.1.275+ in a session).
- Auto-update is **off** by default for third-party marketplaces.
- Sources: https://code.claude.com/docs/en/plugins/install.md, https://code.claude.com/docs/en/plugins/publish.md

**Approval and listing.**
- **Own marketplace.** No review: "Once the file is in the repository, the plugin is published, with no submission form." (https://code.claude.com/docs/en/plugins/publish.md)
- **Anthropic's directory** (claude.ai, Cowork, and Claude Code via `<name>@synced`):
  - Submit at https://claude.ai/directory/manage → "Plugin bundle". It needs a paid claude.ai plan (an Owner on Team/Enterprise) and a GitHub account connected with push access.
  - The plugin can be a folder in a larger repo: "give that folder as the plugin path." The repo must be public before go-live.
  - Every version gets automated validation and a security scan, and a person reviews a new listing.
  - Limit of 10 submissions per org per day. Only one org can hold a given repo/folder.
  - Sources: https://claude.com/docs/plugins/submit, https://claude.com/docs/directory/publish
- **`claude-plugins-official`.** "doesn't take submissions through the directory portal. If you work with an Anthropic partner contact, ask them about an official-marketplace listing." (https://code.claude.com/docs/en/plugins/publish.md). It is added automatically on first interactive start (https://code.claude.com/docs/en/plugins/anthropic-marketplaces.md).
- **`claude-community`** (`anthropics/claude-plugins-community`):
  - It is a read-only mirror "synced nightly from Anthropic's internal review pipeline". Submit via https://clau.de/plugin-directory-submission. Direct PRs are closed automatically.
  - Users must run `claude plugin marketplace add anthropics/claude-plugins-community` and install with `@claude-community`.
  - Source: repo README.
  - **Open question:** whether every approved directory submission also lands in `claude-community` automatically.

**Requirements for Coralogix**
- The cheapest path is to add `.claude-plugin/marketplace.json` (and optionally `plugin.json`) to `coralogix/cx-cli`, with a relative-path entry pointing at the RUM skill folder or at a plugin folder.
- Pick a non-reserved marketplace name. Keep the entry `name` equal to the `plugin.json` `name`.
- Run `claude plugin validate --strict .` in CI. Bump `version`, or omit it so updates follow the commit SHA.
- **Side effect to check.** `npx skills` also discovers skills declared in `.claude-plugin/marketplace.json`/`plugin.json` (see section 2). A root manifest could change what `npx skills add coralogix/cx-cli` installs. Test that before merging.
- **Name collision.** `coralogix/cx-skills` already publishes marketplace `cx-skills` / plugin `coralogix`. Decide whether the RUM skill belongs there instead of starting a second Coralogix marketplace.
- For broad reach, submit the plugin folder to the Anthropic directory (portal). Separately, ask the Anthropic partner contact about `claude-plugins-official`, which already lists Datadog, Sentry, Grafana, Honeycomb, New Relic, and Dynatrace.

## 2. vercel-labs `skills` CLI and public skill indexes

**Install command.** `npx skills add <source> [--skill <name>...] [-a <agent>] [-g] [-y]`. Source forms:
- `owner/repo`
- `owner/repo@ref`
- a full GitHub URL
- a `.../tree/<branch>/<path>` URL
- GitLab, Azure, or any git URL
- a local path

The shorthand `owner/repo/path/to/skill` is also accepted. The source code comment reads "GitHub shorthand: owner/repo, owner/repo/path/to/skill".
- Docs: https://github.com/vercel-labs/skills/blob/main/docs/commands.md
- Parser: https://github.com/vercel-labs/skills/blob/main/src/source-parser.ts

**Discovery.**
- It searches `skills/`, `skills/.curated/`, `.agents/skills/`, `.claude/skills/`, and about 40 other agent directories, up to 3 levels deep. It also reads skills declared in `.claude-plugin/marketplace.json`/`plugin.json`, and custom `paths` in a root `skills.sh.json`. If none are found, it searches recursively.
- `metadata.internal: true` hides a skill unless `INSTALL_INTERNAL_SKILLS=1` is set.
- Source: https://github.com/vercel-labs/skills/blob/main/docs/creating-skills.md
- This explains why cx-cli installs with the `/skills` subpath: it keeps the root `.claude/skills/` dev skills out.

**Per-agent install paths** (https://github.com/vercel-labs/skills/blob/main/docs/agents.md):

| Agent | Project path | Global path |
|---|---|---|
| Claude Code | `.claude/skills/` | `~/.claude/skills/` |
| Codex | `.agents/skills/` | `~/.agents/skills/` |
| Cursor | `.agents/skills/` | `~/.cursor/skills/` |

**skills.sh listing.**
- No submission step. "Skills appear on the leaderboard automatically through anonymous telemetry when users run npx skills add <owner/repo>." (https://skills.sh/docs/faq)
- Telemetry sends repo and skill identifiers only for repos GitHub confirms are public (https://github.com/vercel-labs/skills/blob/main/docs/troubleshooting.md).
- No documented removal or rename process. **Open question:** how to purge stale names that still show on https://skills.sh/coralogix/cx-cli. A contact form exists at skills.sh/contact.

**Agent Skills spec** (https://agentskills.io/specification):
- Required: `name`. It must be 1–64 chars of `a-z`, `0-9`, and `-`, with no leading, trailing, or doubled hyphen, and it **must match the parent directory name**.
- Required: `description`, 1–1024 chars.
- Optional: `license`.
- Optional: `compatibility`, at most 500 chars.
- Optional: `metadata`, a **string→string** map, so `version: "0.1.0"` must be quoted.
- Optional and experimental: `allowed-tools`.
- Keep `SKILL.md` under 500 lines. Use the optional `scripts/`, `references/`, and `assets/` directories.
- Validate with `skills-ref validate ./my-skill`.
- **Note:** several cx-cli skills have no `metadata.version` (for example `coralogix-rum-setup`).

**Other indexes.** Not researched against official docs, so all of these are open questions:
- skillsmp.com
- claude-plugins.dev
- tessl.io (Coralogix already publishes `coralogix/opentelemetry-skills` there)

**Requirements for Coralogix**
- Nothing to submit. `npx skills add coralogix/cx-cli/skills --skill coralogix-rum-setup` already works, and the listing appears once the skill is installed.
- Make sure `name` equals the folder name. Quote `metadata` values.
- Do not add a root `.claude-plugin` manifest that exposes the `.claude/skills` dev skills.

## 3. Cursor

**Skills: native support.**
- Discovery paths: `.agents/skills/`, `.cursor/skills/`, `~/.agents/skills/`, and `~/.cursor/skills/`. Cursor also loads `.claude/skills/`, `.codex/skills/`, and their `~` equivalents.
- Frontmatter: `name` (must match the folder) and `description`, plus Cursor extras `paths`, `disable-model-invocation`, `icon`, `color`, and `metadata`.
- "Skills aren't imported on their own". They are distributed as plugins, and a GitHub import needs `.cursor-plugin/marketplace.json`.
- Source: https://cursor.com/docs/context/skills

**Plugins.**
- Manifest `.cursor-plugin/plugin.json`. Only `name` (kebab-case) is required. Optional fields include `skills`, `rules`, `mcpServers`, and `variables`.
- Default directories: `skills/`, `rules/` (`.md`/`.mdc`), `mcp.json`, `hooks/hooks.json`, `agents/`, and `commands/`.
- A multi-plugin repo uses `.cursor-plugin/marketplace.json` with `name`, `owner.name`, and `plugins[]` (`name`, `source`, optional `metadata.pluginRoot`).
- Sources: https://cursor.com/docs/plugins, https://cursor.com/docs/reference/plugins
- **Open question:** a nested `source` path in the same repo is implied ("Path to plugin directory") but has no example in the docs.
- Template: https://github.com/cursor/plugin-template

**Install.**
- In Cursor, open **Customize**, find the plugin, select **Install**, and choose project or user scope.
- For local development, put the plugin in `~/.cursor/plugins/local`.
- Teams can add a team marketplace.
- Source: https://cursor.com/docs/plugins

**Cursor Marketplace review.**
- "Every plugin is manually reviewed before it's listed." "All plugins must be open source, and we review each update before publishing."
- Submit at https://cursor.com/marketplace/publish. That page's contents were not retrievable, so the form fields are an **open question**.
- Source: https://cursor.com/docs/plugins

**MCP.**
- Remote `mcp.json`: `{"mcpServers":{"name":{"url":"https://...","headers":{...}}}}` (https://cursor.com/docs/context/mcp).
- Deeplink: `cursor://anysphere.cursor-deeplink/mcp/install?name=$NAME&config=$BASE64_ENCODED_CONFIG`, where the config is JSON.stringify of the server object, then base64-encoded (https://cursor.com/docs/context/mcp/install-links). An https web-link variant was not documented on that page (**open question**).
- The public MCP listing is the Marketplace itself. Community listings go to cursor.directory (https://cursor.com/docs/context/mcp).
- The old Rules UI → Remote Rule (GitHub) path in the cx-skills README is not documented on the current skills page (**open question** whether it still works).

**Requirements for Coralogix**
- The quickest route is to add a `coralogix-rum-setup` skill folder to the already-listed `coralogix/coralogix-cursor-plugin`. Each update is re-reviewed, so no new listing is needed.
- The skill must be open source, `name` must match the folder, and no `..` or absolute paths are allowed.
- Project-level `.agents/skills/` or `.claude/skills/` installs via `npx skills` already work without any marketplace.

## 4. OpenAI Codex

**Skills.**
- Codex scans `.agents/skills` from the CWD up to the repo root, `$HOME/.agents/skills`, and `/etc/codex/skills`, plus bundled system skills. `~/.codex/skills` is not listed.
- `SKILL.md` must have `name` and `description`.
- Optional `agents/openai.yaml` sets `interface` (display_name, icons, brand_color, default_prompt), `policy.allow_implicit_invocation`, and `dependencies.tools`.
- Install with `$skill-installer <name>`, or prompt it to install from a repo.
- Source: https://learn.chatgpt.com/docs/build-skills (redirected from https://developers.openai.com/codex/skills)

**Curated repos.**
- `openai/skills` is **deprecated**: "use the [OpenAI Plugins repository]... follow the Build plugins guide" (https://github.com/openai/skills).
- `openai/plugins` is the curated marketplace `openai-curated` ("Codex official") at `.agents/plugins/marketplace.json`.
- No contribution process is documented in its README. It already contains `plugins/sentry` and `plugins/datadog`, both authored by "OpenAI" per their manifests.
- Source: https://github.com/openai/plugins

**Plugin format.**
- Portable form: `plugin.json` at the plugin root. `.codex-plugin/plugin.json` is a "compatibility fallback". Skills live in `skills/<name>/SKILL.md`.
- Marketplace file locations: `$REPO_ROOT/.agents/plugins/marketplace.json`, `~/.agents/plugins/marketplace.json`, or the legacy-compatible `$REPO_ROOT/.claude-plugin/marketplace.json`.
- Each marketplace entry has `name`, `source` (`local`+`path` / `url` / `git-subdir` / `npm`), `policy.installation`/`authentication`, and `category`.
- Subdirectories are supported ("Keep `source.path` relative to the marketplace root, start it with `./`").
- Users add a marketplace with `codex plugin marketplace add owner/repo` and install via `/plugins` in the CLI or the Plugins Directory in the app. No CLI `install` subcommand is documented.
- Source: https://developers.openai.com/plugins/build/plugins

Minimal example of a curated entry (`openai/plugins` sentry), abbreviated:

```json
{ "name": "sentry", "version": "0.1.2", "description": "...", "skills": "./skills/",
  "interface": { "displayName": "Sentry", "shortDescription": "...", "developerName": "...", "category": "Developer Tools" } }
```

Marketplace entry:

```json
{ "name": "sentry", "source": { "source": "local", "path": "./plugins/sentry" },
  "policy": { "installation": "AVAILABLE", "authentication": "ON_INSTALL" }, "category": "Developer Tools" }
```

**Public directory (ChatGPT and Codex).**
- Submit at https://platform.openai.com/plugins → "Upload new or existing plugin", uploaded as a ZIP. It requires individual or business verification, and an org owner or someone with Apps Management Write.
- Skills-only plugins are allowed and need no MCP test cases or video. MCP plugins need 5 positive and 3 negative test cases, a video, and domain verification.
- Review feedback comes by email. You publish after approval.
- Source: https://developers.openai.com/plugins/deploy/submission

**Requirements for Coralogix**
- With no work, `npx skills add ... -a codex` installs into `.agents/skills`.
- For a Codex marketplace, Codex reportedly reads `.claude-plugin/marketplace.json` as legacy-compatible. Verify by testing `codex plugin marketplace add coralogix/cx-cli`.
- For directory listing, submit a skills-only ZIP through the OpenAI Platform. `coralogix/cx-skills` already has a `.codex-plugin/plugin.json` to model on.

## 5. MCP registries (remote server)

**Official MCP Registry** (preview, so "breaking changes or data resets may occur").
- Generate `server.json` with `mcp-publisher init`. It has `$schema` (`.../schemas/2025-12-11/server.schema.json`), `name`, `description`, `version`, `repository`, and `packages[]` or `remotes[]`.
- Log in with one of:
  - `mcp-publisher login github`: the name must be `io.github.<user|org>/*`.
  - `mcp-publisher login dns --domain coralogix.com --private-key ...`: needs the TXT record `coralogix.com. IN TXT "v=MCPv1; k=ed25519; p=<pubkey>"`.
  - `mcp-publisher login http`: needs `/.well-known/mcp-registry-auth`.
- Domain auth gives names of the form `com.coralogix/*`.
- Publish with `mcp-publisher publish`. There is no human review step in the docs.
- Sources: https://modelcontextprotocol.io/registry/quickstart, https://modelcontextprotocol.io/registry/authentication
- **Coralogix status:** already published as `com.coralogix/mcp-server` v2.0.0, a streamable-http remote at `https://api.{region}.coralogix.com/mgmt/api/v1/mcp`. No action is needed beyond version bumps.

**GitHub MCP Registry** (github.com/mcp).
- Per GitHub's blog, servers are published to the open registry first, then you "email partnerships@github.com and request for your server to be included". Self-publication was listed as future work.
- Source: https://github.blog/ai-and-ml/generative-ai/how-to-find-install-and-manage-mcp-servers-with-the-github-mcp-registry/
- **Open question:** whether Coralogix is listed. The `api.mcp.github.com/v0/servers` search and pagination parameters did not filter, so this check was inconclusive.

**Cursor.** MCP servers are listed through the Cursor Marketplace as plugins, where Coralogix is already listed (see section 3), plus the community site cursor.directory.

**Smithery.** As of 2026-10-31, Smithery's proxy, CLI, API, and accounts have shut down. It remains "a public (non-proxied) MCP registry" with "a curated set of verified servers". The submission route is now an **open question** (https://smithery.ai/future banner; docs pages return 404).

**mcp.so.** Form at https://mcp.so/submit with types MCP Server or Remote Server and a Repository URL. A paid option ($39) skips review. Coralogix listing status was not confirmed.

**PulseMCP.** "We are not accepting new MCP server or client submissions right now". It recommends publishing to the official registry, which it "will pick up automatically once we are back" (https://www.pulsemcp.com/submit).

**Anthropic connector directory.**
- Submit at https://claude.ai/directory/manage → "MCP connector".
- Requirements: a remote HTTPS server, OAuth 2.0, `title` plus `readOnlyHint`/`destructiveHint` on every tool, docs and privacy URLs, and a reviewer test account.
- Listings start as Community by default. A per-customer-URL listing must be Verified before it can publish.
- Source: https://claude.com/docs/connectors/building/submission
- **Coralogix status:** already "Anthropic verified" at https://claude.com/connectors/coralogix.
- Anthropic recommends submitting both the connector and a plugin bundle whose skills use it, then pairing the two listings (https://claude.com/docs/directory/publish). That fits a RUM-setup skill plugin.

**Requirements for Coralogix**
- MCP registry presence is largely done (official registry, Anthropic, Cursor).
- Remaining items: ask GitHub (partnerships@github.com) to include `com.coralogix/mcp-server`, and optionally list on mcp.so.

## 6. Datadog and Sentry quick check

Source: catalog files as of the last `claude-plugins-official` commit, 2026-11-04.

**`datadog-labs/agent-skills`**
- **Not** in `claude-plugins-official` or `claude-community`.
- Datadog's listed Claude plugin is a different repo, `datadog-labs/claude-code-plugin` (MCP-based, category monitoring), which appears in both.
- `agent-skills` is its own self-hosted marketplace: `claude plugin marketplace add datadog-labs/agent-skills` → `agent-skills@datadog-labs`, with `source: "./"` and an explicit `skills` array. It also includes `dd-instrument-rum` and `dd-browser-sdk`.
- Cursor Marketplace "Datadog" is built from `datadog-labs/cursor-plugin`, not from agent-skills.
- Sources: https://github.com/datadog-labs/agent-skills, https://cursor.com/marketplace/datadog

**`getsentry/sentry-for-ai`**
- **Not** listed directly. Its README says it is a "skill *source* repository — not something you install directly". CI builds per-agent distribution repos from it.
- `claude-plugins-official` lists `sentry` (`getsentry/plugin-claude`) and `sentry-cli` (git-subdir `getsentry/cli`, `packages/cli/plugins/sentry-cli`).
- Cursor Marketplace "Sentry" comes from `getsentry/plugin-cursor`.
- Sources: https://github.com/getsentry/sentry-for-ai, https://cursor.com/marketplace/sentry

**Pattern.** Vendors ship one skill source and separate thin per-agent distribution repos or folders. `git-subdir` sources such as Grafana, Honeycomb, and `sentry-cli` show that the official marketplace accepts a subfolder of a larger repo.

## Open questions (summary)

1. Does every directory-approved plugin also land in `claude-community`?
2. How does a vendor get into `claude-plugins-official`? The docs say to go through an Anthropic partner contact.
3. What does the Cursor Marketplace publish form ask for, and does a nested `source` path work in `.cursor-plugin/marketplace.json`?
4. Does Codex honor `.claude-plugin/marketplace.json` in cx-cli in practice?
5. How do you remove stale skill names from skills.sh?
6. What are the current submission routes for Smithery (post-Arcade) and mcp.so, and is Coralogix listed on either?
7. Is Coralogix in the GitHub MCP Registry?
8. Is there an existing Coralogix *plugin* listing in the Anthropic directory? The connector exists. `claude.com/plugins/coralogix` returned 404, which is inconclusive.
