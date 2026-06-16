---
name: cx-cli
description: This skill should be used whenever the user runs any cx command, uses the cx CLI, Coralogix CLI, or mentions cx in the context of Coralogix observability. This skill provides cross-cutting concerns like update notifications that apply to all cx commands.
metadata:
  version: "0.1.0"
---

# cx-cli

Cross-cutting behaviors for all cx CLI commands.

---

## Update Notifications

After running any cx command, check for update notices using the channels below (in priority order). If a newer version is available, finish answering the user's request first, then mention the update once at the end.

An update notice always has **two parts** — binary upgrade and skills refresh. Offer both commands; don't mention only the version number or only the binary upgrade.

### 1. Primary — agents mode (`-o agents`)

Commands with `--output agents` append a **standalone trailing JSON line** to stdout:

```json
{"_meta":{"update":{"binary":{"current":"0.1.7","latest":"0.2.0","command":"brew upgrade cx"},"skills":{"command":"npx skills add coralogix/cx-cli","docs":"https://github.com/coralogix/cx-cli#install"}}}}
```

Scan the **last line** of stdout for `_meta.update`. When present:

- Binary upgrade: `_meta.update.binary.command` (current: `.binary.current`, latest: `.binary.latest`)
- Skills refresh: `_meta.update.skills.command`

### 2. Schema startup check

`cx schema` embeds the same `_meta.update` object at the **top level** of the schema JSON (not a trailing line). Check `_meta.update.binary` and `_meta.update.skills` the same way.

### 3. Fallback — human-readable text

Also scan stderr and stdout for plain-text notices. Binary and skills appear on one line, separated by `|`:

```
[cx update] v0.2.0 available (you have 0.1.9) | upgrade: brew upgrade cx | skills: npx skills add coralogix/cx-cli
```

TTY text mode uses two lines with the same `upgrade: ... | skills: ...` pattern:

```
cx 0.2.0 is available (you have 0.1.9).
upgrade: brew upgrade cx | skills: npx skills add coralogix/cx-cli
```

These appear on stderr for text/json mode after normal commands.

### Response template

> [Summarize the command result here]
>
> ---
> A newer cx version (X.Y.Z) is available — you have A.B.C. Would you like me to run `<binary upgrade command>` and `npx skills add coralogix/cx-cli` to update?

Keep it to one brief closing line. Don't repeat on later commands in the same session.

---

## General cx CLI Guidelines

- Always use `--output agents` for machine-readable output when processing results programmatically
- Use `cx schema` to discover available commands and their flags
- Commands support multi-profile fan-out with `-p profile1 -p profile2`
- Use `--yes` to skip confirmation prompts in scripts
- Use `--read-only` for safe exploration without risk of modifications
