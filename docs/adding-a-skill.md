# Adding a Skill

> Step-by-step guide for creating a user-facing skill in `skills/`. Read [adding-a-command.md](adding-a-command.md) first if you're adding a new CLI command — every command needs a skill, and the command guide links here.

## Directory structure

Each skill lives in its own directory under `skills/`:

```
skills/
├── README.md                      ← public catalog (update when adding a skill)
├── your-domain/
│   ├── SKILL.md                   ← required: frontmatter + body
│   └── references/                ← optional: deep-dive reference files
│       └── your-reference.md
```

| File | Required | Purpose |
|------|----------|---------|
| `SKILL.md` | Yes | Main skill definition — frontmatter metadata + markdown instructions |
| `references/*.md` | No | Dense reference material (schemas, enum catalogs, syntax guides) |

Directory name must be **kebab-case** and match the frontmatter `name` field (e.g., `cx-alerts/` → `name: cx-alerts`).

---

## SKILL.md frontmatter

Every `SKILL.md` starts with YAML frontmatter:

```yaml
---
name: cx-your-domain
description: This skill should be used when the user asks to "verb phrase 1", "verb phrase 2", "verb phrase 3", or wants to <broad intent summary> using the cx CLI.
version: 0.1.0
---
```

| Field | Convention |
|-------|------------|
| `name` | kebab-case, matches the directory name |
| `description` | Trigger phrase list — this is how agents decide when to activate the skill |
| `version` | Semver, start at `0.1.0` |

The `description` field is how agents decide when to activate a skill. Follow the pattern used by existing skills — see `skills/cx-alerts/SKILL.md` and `skills/metrics-query/SKILL.md` for examples.

---

## Reference files

Use `references/` for dense reference material that would bloat `SKILL.md` but that agents need when constructing specific payloads or queries.

| Put in SKILL.md | Put in references/ |
|------------------|--------------------|
| CLI commands, flags, basic usage | Complete schema/enum reference (>100 lines) |
| Workflows and investigation steps | Language syntax guides (PromQL, DataPrime) |
| Common examples | Exhaustive field catalogs |

**Rule of thumb:** if it's >100 lines of pure reference material, move it to `references/`.

### Existing reference files

| Skill | Reference file | Content |
|-------|---------------|---------|
| `cx-alerts` | `alert-schemas.md` | JSON schemas for all 12 alert types, enum values |
| `cx-metrics-query` | `promql-guidelines.md` | PromQL value types, counter vs gauge, histogram patterns |
| `cx-dataprime` | `dataprime-reference.md` | Complete DataPrime language reference |
| `cx-query-logs` | `advanced-usage.md` | Investigation workflow, field discovery, advanced queries |
| `cx-query-spans` | `advanced-usage.md` | Span-specific investigation patterns |
| `cx-rum` | `rum-fields.md` | Complete RUM field catalog by event type |

### Naming and linking

- File names: `lowercase-kebab-case.md` (e.g., `alert-schemas.md`, `promql-guidelines.md`)
- Link from SKILL.md at the bottom:

```markdown
## Additional resources

### Reference files

- **`references/your-reference.md`** — One-line description of what it contains
```

---

## Updating skills/README.md

Add a row to the "Available Skills" table in `skills/README.md`:

```markdown
| `your-domain` | One-line description of what the skill covers |
```

Also add a representative user query to the "Usage" section's example list if it illustrates a new use case.

---

## Handling large output

If the data your skill handles is potentially very large (e.g., dashboard JSON, full alert definitions), the command should output to a file rather than stdout. In this case, the corresponding skill should document the expected JSON format so agents can easily parse and script around the output.

For example, if `cx your-domain get <id> -o json` produces large payloads, the skill should instruct agents to pipe output to a file and describe the JSON structure they can expect.

---

## Complete template

Copy this as a starting point for `skills/your-domain/SKILL.md`:

```markdown
---
name: cx-your-domain
description: This skill should be used when the user asks to "list items", "inspect an item", "check item status", "find items by name", "investigate item issues", "query your-domain data", or wants to interact with YourDomain resources using the cx CLI.
version: 0.1.0
---

# YourDomain Skill

Use this skill to query and manage YourDomain resources using the `cx your-domain` CLI commands.

## CLI commands

| Command | Purpose | Key flags |
|---|---|---|
| `cx your-domain list` | List all items | `--name <filter>` |
| `cx your-domain get <id>` | Get a single item by ID | — |

**Output format:** append `-o json` or `-o agents` for machine-readable output.

**Multi-profile:** use `-p <profile>` (repeatable) to target multiple profiles.

## Workflow

1. Start with `cx your-domain list` to see available items
2. Use `cx your-domain get <id> -o json` for full details
3. Use `-o json | jq '...'` for filtering and transformation

## Key principles

- **Use `-o json` with `jq`** for filtering and transformation
- **Multi-profile queries** add a Profile column automatically
- **Always verify** — confirm operations with a follow-up list or get
```

**Reference implementations:** `skills/cx-alerts/SKILL.md` (REST-based command with rich examples) and `skills/metrics-query/SKILL.md` (investigation-oriented workflow).

---

## PR checklist

```markdown
## Checklist

### Skill definition
- [ ] `skills/your-domain/SKILL.md` — valid frontmatter (name, description, version)
- [ ] Frontmatter `description` includes 10+ trigger phrases covering commands, intents, and synonyms
- [ ] Body includes CLI Commands table, workflow, and key principles

### Reference files (if applicable)
- [ ] `skills/your-domain/references/` — deep-dive reference material
- [ ] SKILL.md links to reference files in "Additional Resources" section

### Integration
- [ ] `skills/README.md` — new skill added to the "Available Skills" table
```
