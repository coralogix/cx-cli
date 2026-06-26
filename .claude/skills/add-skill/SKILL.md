---
name: add-skill
description: Use this skill when the user asks to "add a skill", "create a skill", "new skill", "write a skill for", "skill for cx <something>", "add a user-facing skill", "create a SKILL.md", "teach an agent how to use", "agent skill for", "document a command as a skill", "make a skill", "add skill to skills/", "create agent instructions for", "build a skill", or wants to create a new user-facing skill in the skills/ directory that teaches AI agents how to use a cx CLI command. Use this even when the user is following the add-command workflow and reaches the skill creation step.
metadata:
  version: "0.1.0"
  internal: true
---

# Add a User-Facing Skill

End-to-end workflow for creating a skill in `skills/` that teaches AI agents how to use a cx CLI command. This skill is often invoked from `add-command` Step 6, but can also be used standalone when documenting an existing command.

`contributing/adding-a-skill.md` has the full reference guide and a copy-pasteable template. Read it alongside this workflow.

## Step 0: Understand the Domain

Before writing anything, answer these questions:

1. **What cx command are you documenting?** Identify the command and all its subcommands (e.g., `cx alerts list`, `cx alerts get`, `cx alerts create`).
2. **What can the user do with it?** List the key operations, flags, and output formats supported.
3. **Who is the audience?** Skills are consumed by AI agents, not humans directly. Write instructions an agent can follow step-by-step to accomplish a user's goal.
4. **Is there dense reference material?** Schemas, enum catalogs, field references, or language syntax exceeding ~100 lines belong in a separate `references/` file.

## Step 1: Read Existing Skills

Before writing any skill, read existing ones to internalize the project's patterns. Agents that study existing skills first produce consistent, high-quality results rather than inventing new formats.

**Always read:**
- `contributing/adding-a-skill.md` - full guide with directory structure, frontmatter conventions, reference file rules, and a copy-pasteable template
- `skills/README.md` - the public catalog you'll update in Step 4

**Study at least two existing skills:**

| Skill | Why study it |
|-------|-------------|
| `skills/cx-alerts/SKILL.md` | REST-based command with rich examples, JSON payloads, investigation workflow; uses both shared and skill-local reference files |
| `skills/cx-telemetry-querying/SKILL.md` | Gateway skill that loads shared reference files per pillar — good model for cross-pillar routing and reference-loading patterns |
| `skills/cx-dashboards/SKILL.md` | Complex multi-step workflow; uses shared references (dataprime, promql, logs, spans) plus skill-local references |
| `skills/cx-cost-optimization/SKILL.md` | Workflow skill covering 5 commands unified by "reduce costs" intent — good model for multi-command skills |

Pick the two closest to what you're building and read them completely.

## Step 2: Create Directory and Write SKILL.md

Create `skills/cx-<domain-name>/SKILL.md`. All skill directories use the `cx-` prefix. Use the directory structure, frontmatter format, and templates from `contributing/adding-a-skill.md` § "Directory Structure" through "Complete Template" (single-command) or § "Workflow Skills" (multi-command).

### Writing effective trigger descriptions

The `description` frontmatter field is the primary trigger mechanism - agents use it to decide when to activate a skill. This is the single most important line in the file. Write at least **10 trigger phrases** covering:
- Explicit command names (`"list alerts"`, `"get alert by ID"`)
- User intents (`"investigate alert failures"`, `"debug noisy alerts"`)
- Synonyms and variations (`"check"`, `"find"`, `"search"`, `"analyze"`)
- Domain jargon (`"SLO breach"`, `"error budget"`)

### Body structure

Follow this order (see `contributing/adding-a-skill.md` § "Complete Template" for a starting point):

1. **Title and intro** - one sentence explaining what the skill covers and which `cx` commands it uses
2. **CLI Commands table** - `| Command | Purpose | Key flags |` for every subcommand. Include `-o json`/`-o agents` and `-p <profile>` notes.
3. **Workflow / Investigation steps** - numbered steps an agent should follow. Start with discovery, end with verification.
4. **Key Principles** - 4-6 bullet points: use `-o json` with `jq`, multi-profile behavior, always verify, etc.
5. **Additional Resources** - links to `references/` files and related skills

### Style guidelines

- Write for an AI agent - be explicit about what to run and in what order
- Use code blocks for every CLI invocation
- Include realistic examples with actual flag values, not placeholders
- Show `jq` filtering examples when the command supports `-o json`
- Keep SKILL.md under ~200 lines; move dense material to `references/`

## Step 3: Add Reference Files and Update README

Follow `contributing/adding-a-skill.md` § "Reference Files" for when and how to create `references/` files, and § "Updating skills/README.md" for adding the skill to the public catalog.

### Shared vs. skill-local references

Before creating a new reference file, check whether the content is language-level or telemetry-pillar material:

- **Language guides** (DataPrime syntax, PromQL guidelines) → `skills/shared/`
- **Telemetry-pillar how-tos** (logs querying, spans querying, metrics workflow, RUM) → `skills/shared/`
- **Skill-specific schemas or templates** (alert JSON schemas, dashboard widget templates) → `skills/cx-your-domain/references/`

If your skill needs a file from `skills/shared/`:
1. Add your skill and file list to `scripts/sync-shared-references.sh`
2. Run `bash scripts/sync-shared-references.sh` — it copies the file(s) into your `references/`
3. Commit the sync script change and the generated `references/` copies together

If you're adding a new shared reference:
1. Create it in `skills/shared/`
2. Register it in the sync script for all consuming skills
3. Run the script to generate copies, then commit everything

## Step 4: Verify

1. **Frontmatter** - `name` matches directory, `description` has 10+ trigger phrases, `metadata.version` is `"0.1.0"`
2. **Body completeness** - has CLI Commands table, workflow steps, key principles, and additional resources (if references exist)
3. **Reference links** - any `references/` paths in SKILL.md point to files that actually exist
4. **README updated** - `skills/README.md` has the new row
5. **No bloat** - SKILL.md doesn't duplicate >100 lines of material that belongs in `references/`
6. **Shared references synced** - if the skill uses files from `skills/shared/`, confirm `bash scripts/sync-shared-references.sh` was run and the generated `references/` copies are committed
7. **Agent readability** - would an AI agent know exactly what to do after reading this skill? If not, add missing steps or examples

For advanced trigger optimization, use the `/skill-creator` skill to run eval-driven description testing and iterate on your trigger phrases.

Use the PR checklist from `contributing/adding-a-skill.md` § "PR Checklist" in your PR description.
