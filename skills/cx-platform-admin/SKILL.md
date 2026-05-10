---
name: cx-platform-admin
description: >
  Use this skill when the user asks "who has access", "audit permissions",
  "check user roles", "list API keys", "review access controls",
  "rotate API keys", "create API key", "delete expired keys", "send data keys",
  "IP allowlist", "IP access restrictions",
  "check IP whitelist", "add user", "deactivate user", "manage team groups",
  "user permissions", "role-based access", "manage scopes", "system roles",
  "API key admin", "team member keys", "group membership",
  or wants to audit, manage, or configure access controls for a Coralogix account.
metadata:
  version: "0.1.0"
---

# Platform Admin Skill

Use this skill for managing access, authentication, and authorization in Coralogix. It covers API key management, role and scope definitions, user administration, team groups, and IP access restrictions.

---

## Destructive Operation Safety

All write operations (create, update, delete, set-idp, set-active, set-status) require interactive confirmation. The CLI will prompt before executing. To skip the prompt in scripts, pass `--yes`.

**IMPORTANT: NEVER pass `--yes` without explicit user approval.** Before executing any write operation:
1. Describe the exact operation to the user (what will be created/modified/deleted)
2. Wait for the user to confirm
3. Only then execute with `--yes`

Read-only operations (list, get, search, system, sp-params, send-data-keys) do not require confirmation and can be run freely.

### Read-Only Mode

Use `--read-only` (or `CX_READ_ONLY=1`) to block all write operations at the CLI level. This is useful for safe exploration - you can query any IAM resource without risk of accidental modifications.

### Agent Mode

When running inside an AI agent (Claude Code, Cursor, Codex, etc.), cx automatically detects the agent environment and fails fast on write operations instead of hanging on a stdin prompt. The error message instructs you to get user confirmation first, then re-run with `--yes`.

---

## CLI Commands

### API Keys

| Command | Purpose |
|---|---|
| `cx iam api-keys list` | List all API keys |
| `cx iam api-keys get <id>` | Get a single API key |
| `cx iam api-keys create --from-file` | Create an API key |
| `cx iam api-keys update --from-file <id>` | Update an API key |
| `cx iam api-keys delete <id>` | Delete an API key |
| `cx iam api-keys send-data-keys` | List send-data API keys |
| `cx iam api-keys admin list` | List all team members' keys |
| `cx iam api-keys admin delete --ids <id1> <id2>` | Bulk delete keys |
| `cx iam api-keys admin set-status --ids <id1> --active true/false` | Activate/deactivate keys |

### Roles & Scopes

| Command | Purpose |
|---|---|
| `cx iam roles list` | List custom roles |
| `cx iam roles get <id>` | Get a role definition |
| `cx iam roles create --from-file` | Create a custom role |
| `cx iam roles update --from-file <id>` | Update a custom role |
| `cx iam roles delete <id>` | Delete a custom role |
| `cx iam roles system` | List system (built-in) roles |
| `cx iam scopes list` | List all scopes |
| `cx iam scopes get <id>` | Get a scope definition |
| `cx iam scopes create --from-file` | Create a scope |
| `cx iam scopes update --from-file` | Update a scope |
| `cx iam scopes delete <id>` | Delete a scope |

### Users & Groups

| Command | Purpose |
|---|---|
| `cx iam users search` | Search users (optional `--query`, `--status`) |
| `cx iam users get <user-id>` | Get a single user |
| `cx iam users create --from-file` | Create user(s) |
| `cx iam users update --from-file` | Update user(s) |
| `cx iam users set-status --user-ids <id> --status ACTIVE/INACTIVE` | Activate/deactivate users |
| `cx iam groups list` | List all team groups |
| `cx iam groups get <id>` | Get a group by ID |
| `cx iam groups get-by-name <name>` | Get a group by name |
| `cx iam groups users <group-id>` | List users in a group |
| `cx iam groups create --from-file` | Create a group |
| `cx iam groups update --from-file <id>` | Update a group |
| `cx iam groups delete <id>` | Delete a group |

### IP Access

| Command | Purpose |
|---|---|
| `cx iam ip-access get` | Get IP access settings |
| `cx iam ip-access create --from-file` | Create IP access rules |
| `cx iam ip-access update --from-file` | Update IP access rules |
| `cx iam ip-access delete` | Delete IP access settings |

All commands support `-o json` for structured output and `-p <profile>` for profile selection.

---

## Access Audit Workflow

Use this workflow to produce a comprehensive access report:

### Step 1: List All Users

```bash
cx iam users search -o json
cx iam users search -o json | jq '[.[] | {id, name: .user_name, status, role_ids}]'
```

### Step 2: List Roles

```bash
cx iam roles list -o json
cx iam roles system -o json
```

Cross-reference user role IDs with role definitions to understand permissions.

### Step 3: List Groups and Memberships

```bash
cx iam groups list -o json
cx iam groups list -o json | jq '[.[] | {id, name, member_count: (.members | length)}]'
```

For each group, check members:

```bash
cx iam groups users <group-id> -o json
```

### Step 4: Inventory API Keys

```bash
cx iam api-keys list -o json
cx iam api-keys admin list -o json
cx iam api-keys send-data-keys -o json
```

Identify old or unused keys:

```bash
cx iam api-keys list -o json | jq '[.[] | {id, name, created_at, active}] | sort_by(.created_at)'
```

### Step 5: Check IP Restrictions

```bash
cx iam ip-access get -o json
```

### Step 6: Cross-Reference

Produce a summary: which users have admin roles, which API keys are old, which groups have broad access.

---

## API Key Rotation

Safe key rotation workflow:

1. **List current keys:** `cx iam api-keys list -o json`
2. **Identify keys to rotate:** filter by age or name
3. **Create replacement key:** `cx iam api-keys create --from-file new-key.json --yes` (after user approval)
4. **Deploy the new key** to all systems using the old key
5. **Verify the new key works** in all integrations
6. **Delete the old key:** `cx iam api-keys delete <old-key-id> --yes` (after user approval)

> **WARNING:** Never delete an API key before its replacement is deployed and verified. Deleting an active key immediately breaks all integrations using it.

---

## Safety Callouts

> **Deleting API keys** breaks any integration using that key immediately. Always create a replacement first.

> **Deactivating users** (`cx iam users set-status --status INACTIVE`) takes effect immediately. The user loses access with no grace period.

> **Deleting IP access rules** (`cx iam ip-access delete`) removes all IP restrictions immediately, potentially exposing the account.

---

## Key Principles

- **Audit before modifying** - run the full access audit workflow before making changes
- **Never delete keys without replacement** - create new key → deploy → verify → delete old
- **Use `-o json` for structured reports** - enables jq filtering for precise access analysis
- **Multi-profile for cross-environment audits** - use `-p <profile>` or `--all-profiles` to audit staging + production
- **Template from existing** - `cx iam roles get <id> -o json > role.json` before creating new roles

---

## Related Skills

- **`cx-cost-optimization`** - review what API keys are used for and whether they're still needed
