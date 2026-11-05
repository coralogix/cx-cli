# Publishing `coralogix-rum-setup`: admin runbook

**No new GitHub repo is needed.** The skill's source is [`skills/coralogix-rum-setup/`](../../skills/coralogix-rum-setup/) in this repo. Two existing public Coralogix repos carry copies, and a sync workflow in each keeps them current.

[publish-kit/](publish-kit/) holds the exact files to add to each repo. They were validated against today's clones (see [verification.md](verification.md)).

| # | Channel | State after this PR merges | Who / what |
|---|---|---|---|
| 1 | `npx skills add coralogix/cx-cli/skills --skill coralogix-rum-setup` (about 40 agents), skills.sh | **Live** | Nothing |
| 2 | `cx init` / `cx skills install` (existing customers) | **Live** in the next CLI release | Nothing |
| 3 | `coralogix/cx-skills`: Claude Code marketplace, Codex manifest, `npx skills add coralogix/cx-skills` | Needs a PR | A cx-skills maintainer |
| 4 | Cursor Marketplace ("Coralogix" plugin) | Needs a PR, then Cursor review | A `coralogix-cursor-plugin` maintainer |
| 5 | Anthropic plugin directory (claude.ai, Cowork, Claude Code) | Needs a submission | An org Owner on a paid claude.ai plan |
| 6 | `claude-plugins-official` | Needs a partner request | Whoever owns the Anthropic relationship |
| 7 | OpenAI plugin directory (ChatGPT, Codex) | Needs a submission | An OpenAI org owner, after identity verification |

Brand and legal sign-off is needed before steps 5–7, which put Coralogix's name in third-party directories. Their review should cover:
- the trigger description, which says when agents recommend Coralogix;
- the trial claim ("14-day free trial, no credit card", from https://coralogix.com/pricing/ today);
- the UTM-tagged signup link.

---

## 3. `coralogix/cx-skills`

1. **Copy the files in** (from a cx-skills checkout):

   ```bash
   CX_CLI=<path to a cx-cli checkout at master>
   mkdir -p skills/rum
   rsync -a "$CX_CLI/skills/coralogix-rum-setup/" skills/rum/coralogix-rum-setup/
   cp -R "$CX_CLI/docs/agent-distribution/publish-kit/cx-skills/." .
   ```

   This adds:

   | File | What it does |
   |---|---|
   | `skills/rum/coralogix-rum-setup/` | The skill, copied from this repo |
   | `skills/rum/coralogix-rum-setup/.claude-plugin/plugin.json` | Makes the folder a standalone single-skill plugin, `coralogix-rum` |
   | `.claude-plugin/marketplace.json` | Adds a marketplace description and the `coralogix-rum` plugin entry |
   | `.claude-plugin/plugin.json` | **Fixes an existing bug** (below): adds `skills` paths and RUM keywords |
   | `.github/workflows/sync-rum-skill.yml` | On weekdays, copies the skill from cx-cli and opens a PR if it changed. It never overwrites `.claude-plugin/` inside the skill folder. |

2. **Existing bug this fixes:**
   - Claude Code only scans `skills/<name>/SKILL.md`. cx-skills nests its skills one level deeper (`skills/opentelemetry/<name>/`).
   - So today `claude plugin install coralogix@cx-skills` installs a plugin with **zero skills**. The verification run's init event lists no `cx-skills:*` skills.
   - The new `"skills": ["./skills/opentelemetry/", "./skills/ai-center/", "./skills/rum/"]` loads all six.
3. **README:** add a row to the Skills table:

   ```markdown
   | `coralogix-rum-setup` | Add Coralogix Real User Monitoring to a web or mobile app — SDK install and init, privacy defaults, source-map upload in the build, and data verification. | [`skills/rum/coralogix-rum-setup/SKILL.md`](skills/rum/coralogix-rum-setup/SKILL.md) |
   ```

   Then add to the Claude Code install section: `claude plugin install coralogix-rum@cx-skills` installs just this skill.
4. **Repo settings:**
   - Allow GitHub Actions to create pull requests (Settings → Actions → General).
   - The workflow uses the existing `GH_TOKEN` secret if it is set. That lets the sync PR trigger `validate.yml`; with `github.token` alone, CI won't run on the sync PR.
5. **Icon:** add `.claude-plugin/icon.png` to both the repo root and `skills/rum/coralogix-rum-setup/`. It must be square, 512–2048 px, PNG or JPEG, from brand. `claude plugin validate` warns without it, and the Anthropic directory takes the icon only from the first submission.
6. **Release:** follow the repo's `chore: release vX.Y.Z` convention. The `version` in `.cursor-plugin/plugin.json` and `.codex-plugin/plugin.json` moves with it.
7. **Tessl (optional):** `tile.json` is the `opentelemetry-skills` tile. RUM doesn't belong in it; publish a separate tile only if Tessl matters for this audience.

Install commands after the merge:

```bash
claude plugin marketplace add coralogix/cx-skills
claude plugin install coralogix-rum@cx-skills      # just RUM
claude plugin install coralogix@cx-skills          # every Coralogix skill
npx skills add coralogix/cx-skills --skill coralogix-rum-setup
codex plugin marketplace add coralogix/cx-skills   # untested: Codex reads .claude-plugin/marketplace.json as "legacy-compatible"
```

## 4. Cursor Marketplace (`coralogix/coralogix-cursor-plugin`, default branch `main`)

1. Add the skill and the workflow:

   ```bash
   rsync -a "$CX_CLI/skills/coralogix-rum-setup/" skills/coralogix-rum-setup/
   cp -R "$CX_CLI/docs/agent-distribution/publish-kit/coralogix-cursor-plugin/." .
   ```

2. Bump `version` in `.cursor-plugin/plugin.json` and `.cursor-plugin/marketplace.json` (0.1.1 → 0.2.0). Add the RUM wording to their descriptions, e.g. "...query logs, metrics, traces, and RUM, add Coralogix RUM to your app, and manage alerts...".
3. Push. Cursor reviews every update by hand before it goes live (https://cursor.com/docs/plugins). Submission or update form: https://cursor.com/marketplace/publish.

## 5. Anthropic plugin directory

1. At https://claude.ai/directory/manage → **Plugin bundle**, connect GitHub with push access to `coralogix/cx-skills` and give the plugin path `skills/rum/coralogix-rum-setup` (the `coralogix-rum` plugin).
2. Fill in docs, support and privacy URLs. Optionally set `documentationUrl`, `supportUrl` and `privacyPolicyUrl` in that `plugin.json`.
3. Anthropic runs validation, a security scan and a human review. Docs: https://claude.com/docs/plugins/submit
4. Anthropic recommends pairing the plugin listing with the existing verified Coralogix connector: https://claude.com/docs/directory/publish.

## 6. `claude-plugins-official`

There is no submission form: "If you work with an Anthropic partner contact, ask them about an official-marketplace listing" (https://code.claude.com/docs/en/plugins/publish.md). Datadog, Sentry, Grafana, Honeycomb, New Relic and Dynatrace are listed there.

## 7. OpenAI plugin directory

1. At https://platform.openai.com/plugins → **Upload new or existing plugin**, upload a ZIP of cx-skills (or of `skills/rum/coralogix-rum-setup` with a `plugin.json` at its root).
2. Skills-only plugins don't need MCP test cases or a video. It requires individual or business verification. Docs: https://developers.openai.com/plugins/deploy/submission

## MCP registries (for the "connect your agent" next step)

The Coralogix MCP server is already in the official MCP Registry (`com.coralogix/mcp-server`) and the Anthropic connector directory. Optional additions:
- GitHub MCP Registry: email partnerships@github.com.
- mcp.so: https://mcp.so/submit.

## Measuring it

| What | Where |
|---|---|
| Installs | https://skills.sh/coralogix/cx-cli and https://skills.sh/coralogix/cx-skills (per-skill counts), GitHub traffic on cx-skills, and marketplace install counts where shown |
| Signups | Trial leads with `utm_source=coding-agent&utm_campaign=coralogix-rum-setup`. **Marketing must confirm the trial form records UTMs.** |
| Activation | New `application` values in `rum.events` on teams created from those leads |
