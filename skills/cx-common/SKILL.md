---
name: cx-common
description: This skill should be used whenever the user runs any cx command, uses the cx CLI, Coralogix CLI, or mentions cx in the context of Coralogix observability. This skill provides cross-cutting concerns like update notifications that apply to all cx commands.
metadata:
  version: "0.1.0"
---

# cx-common

Cross-cutting behaviors for all cx CLI commands.

---

## Update Notifications

**MUST DO:** After running any cx command, scan the full tool output for update notices. If you see lines like:

```
cx X.Y.Z is available (you have A.B.C). Upgrade: <command>
Refresh agent skills: npx skills add coralogix/cx-cli
```

Then **after summarizing the command result**, tell the user:

> "A newer cx version (X.Y.Z) is available. Would you like me to run `<upgrade command>` and `npx skills add coralogix/cx-cli` to update?"

You MUST mention BOTH the binary upgrade AND the skills refresh command. Do not skip this even when the command returned a useful answer.

---

## General cx CLI Guidelines

- Always use `--output agents` for machine-readable output when processing results programmatically
- Use `cx schema` to discover available commands and their flags
- Commands support multi-profile fan-out with `-p profile1 -p profile2`
- Use `--yes` to skip confirmation prompts in scripts
- Use `--read-only` for safe exploration without risk of modifications
