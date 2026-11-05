---
name: cx-cli
description: This skill should be used whenever the user runs any cx command, uses the cx CLI, Coralogix CLI, or mentions cx in the context of Coralogix observability. This skill provides cross-cutting concerns like update notifications that apply to all cx commands.
metadata:
  version: "0.2.0"
---

# cx-cli

Cross-cutting behaviors for all cx CLI commands.

---

## Update Notifications

After running any cx command, check for update notices using the channels below (in priority order). If an update is available, finish answering the user's request first, then mention the update once at the end.

There are two kinds of notice:

- **Binary update** — a newer cx version exists. The notice has two parts: the binary upgrade and the skills refresh. Offer both commands, binary first.
- **Skills-only update** — cx itself is current, but the installed agent skills are older than it (typical right after `brew upgrade cx`). Offer only the skills refresh command.

### 1. Primary — toon mode (`-o toon`)

Commands with `--output toon` append a **standalone trailing JSON line** to stdout. Binary update:

```json
{"_meta":{"update":{"binary":{"current":"0.1.7","latest":"0.2.0","command":"brew upgrade cx"},"skills":{"command":"cx skills install --global","docs":"https://github.com/coralogix/cx-cli#install"}}}}
```

Skills-only update (no `binary` key; `installed_for` is the cx version the skills were installed for, or `null` when unknown):

```json
{"_meta":{"update":{"skills":{"command":"cx skills install --global","docs":"https://github.com/coralogix/cx-cli#install","installed_for":"0.1.20"}}}}
```

Scan the **last line** of stdout for `_meta.update`. When present:

- Binary upgrade (only if `_meta.update.binary` exists): `.binary.command` (current: `.binary.current`, latest: `.binary.latest`)
- Skills refresh: `_meta.update.skills.command`

### 2. Schema startup check

`cx schema` embeds the same `_meta.update` object at the **top level** of the schema JSON (not a trailing line). Check `_meta.update.binary` and `_meta.update.skills` the same way.

### 3. Fallback — human-readable text

Also scan stderr and stdout for plain-text notices. A binary update shows binary and skills on one line, separated by `|`:

```
[cx update] v0.2.0 available (you have 0.1.9) | upgrade: brew upgrade cx | skills: cx skills install --global
```

A skills-only update:

```
[cx update] cx agent skills are older than cx 0.1.9 | skills: cx skills install --global
```

TTY text mode uses two lines with the same `upgrade: ... | skills: ...` or `update: ...` pattern:

```
cx 0.2.0 is available (you have 0.1.9).
upgrade: brew upgrade cx | skills: cx skills install --global
```

These appear on stderr for text/json mode after normal commands.

### Response template

Binary update:

> [Summarize the command result here]
>
> ---
> A newer cx version (X.Y.Z) is available — you have A.B.C. Would you like me to run `<binary upgrade command>` and then `<skills command>` to update?

Skills-only update:

> [Summarize the command result here]
>
> ---
> Your cx agent skills are older than your cx version, so some commands may be missing from them. Would you like me to run `<skills command>` to update them?

Keep it to one brief closing line. Don't repeat on later commands in the same session.

---

## General cx CLI Guidelines

- Always use `--output toon` for machine-readable output when processing results programmatically
- Use `cx schema` to discover available commands and their flags
- Commands support multi-profile fan-out with `-p profile1 -p profile2`
- For commands that query or manage Coralogix data, use `--http-timeout <SECONDS>` or `CX_HTTP_TIMEOUT=<SECONDS>` to set an API request deadline.
- Use `--yes` to skip confirmation prompts in scripts
- Use `--read-only` for safe exploration without risk of modifications
