# Adding a Skill

> Step-by-step guide for creating a user-facing skill in `skills/`. Read [adding-a-command.md](adding-a-command.md) first if you're adding a new CLI command - every command must be covered by a skill (either its own or a workflow skill that covers multiple commands), and the command guide links here.

## Directory structure

Each skill lives in its own directory under `skills/`:

```
skills/
├── README.md                      ← public catalog (update when adding a skill)
├── shared/                        ← language guides and telemetry-pillar references (shared across skills)
│   ├── dataprime-reference.md
│   ├── promql-guidelines.md
│   ├── logs-querying.md
│   └── ...
├── cx-your-domain/
│   ├── SKILL.md                   ← required: frontmatter + body
│   └── references/                ← optional: deep-dive reference files
│       └── your-reference.md      ← copies of shared/ files or skill-local references
```

| File | Required | Purpose |
|------|----------|---------|
| `SKILL.md` | Yes | Main skill definition - frontmatter metadata + markdown instructions |
| `references/*.md` | No | Dense reference material (schemas, enum catalogs, syntax guides) |

Directory name must be **`cx-` prefixed kebab-case** and match the frontmatter `name` field (e.g., `cx-alerts/` → `name: cx-alerts`). All skills use the `cx-` prefix for consistency.

---

## SKILL.md frontmatter

Every `SKILL.md` starts with YAML frontmatter:

```yaml
---
name: cx-your-domain
description: This skill should be used when the user asks to "verb phrase 1", "verb phrase 2", "verb phrase 3", or wants to <broad intent summary> using the cx CLI.
metadata:
  version: "0.1.0"
---
```

| Field | Convention |
|-------|------------|
| `name` | kebab-case, matches the directory name |
| `description` | Trigger phrase list - this is how agents decide when to activate the skill |
| `metadata.version` | Semver, start at `0.1.0`; keep repo-specific fields under `metadata` so `agentskills validate` accepts the frontmatter |

The `description` field is how agents decide when to activate a skill. Follow the pattern used by existing skills - see `skills/cx-alerts/SKILL.md` and `skills/cx-telemetry-querying/SKILL.md` for examples.

---

## Reference files

Use `references/` for dense reference material that would bloat `SKILL.md` but that agents need when constructing specific payloads or queries.

| Put in SKILL.md | Put in references/ |
|------------------|--------------------|
| CLI commands, flags, basic usage | Complete schema/enum reference (>100 lines) |
| Workflows and investigation steps | Language syntax guides (PromQL, DataPrime) |
| Common examples | Exhaustive field catalogs |

**Rule of thumb:** if it's >100 lines of pure reference material, move it to `references/`.

### Shared references

Language-level references (DataPrime syntax, PromQL guidelines) and telemetry-pillar references (logs, spans, metrics, RUM querying) live in `skills/shared/` — not in individual skill directories. These files are the canonical source of truth and are distributed to consuming skills by the sync script.

**To use a shared reference in your skill:**
1. Add your skill and the file(s) it needs to `scripts/sync-shared-references.sh`
2. Run `bash scripts/sync-shared-references.sh` to copy the files into your skill's `references/` directory
3. Commit both the sync script change and the generated `references/` copies

**To add a new shared reference:**
1. Create the file in `skills/shared/`
2. Register it in `scripts/sync-shared-references.sh` for every skill that needs it
3. Run the script to generate copies, then commit everything together

| Shared file | Content |
|-------------|---------|
| `skills/shared/dataprime-reference.md` | Complete DataPrime language reference (query structure, prefixes, operators, functions) |
| `skills/shared/promql-guidelines.md` | PromQL value types, counter vs gauge, histogram patterns, cheat-sheet |
| `skills/shared/logs-querying.md` | Log data model, field discovery, investigation workflow, common patterns |
| `skills/shared/spans-querying.md` | Span data model, duration units, error detection, investigation patterns |
| `skills/shared/metrics-querying.md` | Metrics CLI workflow (search → query → range), common patterns, principles |
| `skills/shared/rum-querying.md` | RUM data model, event types, error detection, web vitals queries |
| `skills/shared/rum-fields.md` | Complete RUM field catalog by event type (`$d.cx_rum.*`) |

### Skill-local reference files

Some reference files are unique to one skill and do not belong in `skills/shared/`:

| Skill | Reference file | Content |
|-------|---------------|---------|
| `cx-alerts` | `alert-schemas.md` | JSON schemas for all 12 alert types, enum values |
| `cx-create-dashboard` | `query-syntax.md` | Coralogix dashboard query gotchas and cross-references |
| `cx-create-dashboard` | `widget-templates.md` | Widget JSON templates for all widget types |
| `cx-create-dashboard` | `verification.md` | Live-verification procedure for dashboard queries |
| `cx-create-dashboard` | `deploy.md` | Dashboard deployment procedure via `cx dashboards create` |

### Naming and linking

- File names: `lowercase-kebab-case.md` (e.g., `alert-schemas.md`, `promql-guidelines.md`)
- Link from SKILL.md at the bottom:

```markdown
## Additional resources

### Reference files

- **`references/your-reference.md`** - One-line description of what it contains
```

---

## Updating skills/README.md

Add a row to the "Available Skills" table in `skills/README.md`:

```markdown
| `cx-your-domain` | One-line description of what the skill covers |
```

Also add a representative user query to the "Usage" section's example list if it illustrates a new use case.

---

## Handling large output

If the data your skill handles is potentially very large (e.g., dashboard JSON, full alert definitions), the command should output to a file rather than stdout. In this case, the corresponding skill should document the expected JSON format so agents can easily parse and script around the output.

For example, if `cx your-domain get <id> -o json` produces large payloads, the skill should instruct agents to pipe output to a file and describe the JSON structure they can expect.

---

## Complete template

Copy this as a starting point for a single-command skill (`skills/cx-your-domain/SKILL.md`):

```markdown
---
name: cx-your-domain
description: This skill should be used when the user asks to "list items", "inspect an item", "check item status", "find items by name", "investigate item issues", "query your-domain data", or wants to interact with YourDomain resources using the cx CLI.
metadata:
  version: "0.1.0"
---

# YourDomain Skill

Use this skill to query and manage YourDomain resources using the `cx your-domain` CLI commands.

## CLI commands

| Command | Purpose | Key flags |
|---|---|---|
| `cx your-domain list` | List all items | `--name <filter>` |
| `cx your-domain get <id>` | Get a single item by ID | - |

**Output format:** append `-o json` or `-o agents` for machine-readable output.

**Multi-profile:** use `-p <profile>` (repeatable) to target multiple profiles.

## Workflow

1. Start with `cx your-domain list` to see available items
2. Use `cx your-domain get <id> -o json` for full details
3. Use `-o json | jq '...'` for filtering and transformation

## Key principles

- **Use `-o json` with `jq`** for filtering and transformation
- **Multi-profile queries** add a Profile column automatically
- **Always verify** - confirm operations with a follow-up list or get
```

**Reference implementations:** `skills/cx-alerts/SKILL.md` (REST-based command with rich examples, uses shared + local references) and `skills/cx-telemetry-querying/SKILL.md` (gateway skill that loads shared reference files per pillar).

---

## Workflow skills

Not every skill maps 1:1 to a CLI command. **Workflow skills** cover multiple commands unified by a user intent. For example, `cx-cost-optimization` covers `cx usage`, `cx tco`, `cx retentions`, and `cx archive` - all under the "reduce costs" intent.

When adding a new command, check whether an existing workflow skill already covers it. If so, add your command to that skill rather than creating a new one. If the command serves a new intent not covered by any existing skill, create a workflow skill.

### Workflow skill template

```markdown
---
name: cx-your-workflow
description: >
  Use this skill when the user asks to "intent phrase 1", "intent phrase 2",
  "intent phrase 3", "domain term 1", "domain term 2",
  or wants to <broad intent summary>.
metadata:
  version: "0.1.0"
---

# Your Workflow Skill

Use this skill when <intent description>. It covers <command 1>, <command 2>,
and <command 3>.

## CLI Commands

| Command | Subcommands | Purpose |
|---|---|---|
| `cx command-a` | `list`, `get`, `create` | Step 1 purpose |
| `cx command-b` | `list`, `get`, `update` | Step 2 purpose |

## Workflow

### Step 1: Description
\`\`\`bash
cx command-a list -o json
\`\`\`

### Step 2: Description
\`\`\`bash
cx command-b list -o json
\`\`\`

## Key Principles

- **Always measure before changing**
- **Use `-o json` with `jq`** for structured analysis
- **Verify changes** with follow-up queries

## Related Skills

- **`cx-other-skill`** - description of relationship
```

**Reference implementations:** `skills/cx-cost-optimization/SKILL.md` (5-command workflow) and `skills/cx-incident-management/SKILL.md` (multi-command orchestrator with cross-skill delegation).

---

## PR checklist

```markdown
## Checklist

### Skill definition
- [ ] `skills/cx-your-domain/SKILL.md` - valid Agent Skills frontmatter (`name`, `description`, `metadata.version`)
- [ ] Frontmatter `description` includes 10+ trigger phrases covering commands, intents, and synonyms
- [ ] Body includes CLI Commands table, workflow, and key principles

### Reference files (if applicable)
- [ ] `skills/cx-your-domain/references/` - deep-dive reference material
- [ ] SKILL.md links to reference files in "Additional Resources" section
- [ ] If the skill uses shared references: added to `scripts/sync-shared-references.sh` and ran the script
- [ ] Shared reference copies committed alongside `scripts/sync-shared-references.sh` change

### Integration
- [ ] `skills/README.md` - new skill added to the "Available Skills" table

### Verification
- [ ] CI runs `agentskills validate` for upstream Agent Skills spec validation
- [ ] `scripts/verify-skills.sh` - cx-cli-specific checks pass (triggers, commands, cross-refs, reference sync)
```
