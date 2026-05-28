---
name: cx-observability-setup
description: >
  Use this skill when the user asks to "set up monitoring", "configure observability",
  "onboard new service", "create saved view", "set up notifications",
  "configure webhook", "set up Slack integration", "outgoing webhook",
  "automation action", "webhook for alerts", "create view", "saved view",
  "view folder", "organize dashboards", "install integration",
  "configure extension", "contextual data", "connect external service",
  "create notification connector", "set up email alerts", "configure PagerDuty",
  "notification routing", "deploy extension", "test webhook",
  "notification preset", "test notification", "webhook actions",
  or wants to set up, configure, or manage the observability stack for a service or team.
metadata:
  version: "0.1.0"
---

# Observability Setup Skill

Use this skill when setting up or configuring the observability stack - saved views, webhook integrations, notification channels, and external integrations. This is the "day 1 setup" skill for onboarding a new service or reconfiguring notification pipelines.

---

## CLI Commands

### Views

| Command | Purpose |
|---|---|
| `cx views list` | List all saved views |
| `cx views get <id>` | Get a view definition |
| `cx views create --from-file` | Create a saved view |
| `cx views update <id> --from-file` | Update a saved view |
| `cx views delete <id>` | Delete a saved view |
| `cx views folders list` | List view folders |
| `cx views folders get <id>` | Get a folder |
| `cx views folders create --from-file` | Create a folder |
| `cx views folders update <id> --from-file` | Update a folder |
| `cx views folders delete <id>` | Delete a folder |

### Webhooks

| Command | Purpose |
|---|---|
| `cx webhooks list` | List all outgoing webhooks |
| `cx webhooks get <id>` | Get webhook details |
| `cx webhooks create --from-file` | Create a webhook |
| `cx webhooks update <id> --from-file` | Update a webhook |
| `cx webhooks delete <id>` | Delete a webhook |
| `cx webhooks test <id>` | Test a webhook |
| `cx webhooks types` | List available webhook types |
| `cx webhooks actions list` | List automation actions |
| `cx webhooks actions get <id>` | Get action details |
| `cx webhooks actions create --from-file` | Create an action |
| `cx webhooks actions update --from-file` | Update an action |
| `cx webhooks actions delete <id>` | Delete an action |
| `cx webhooks actions batch --from-file` | Batch execute actions |
| `cx webhooks actions reorder --from-file` | Reorder actions |

### Notifications

| Command | Purpose |
|---|---|
| `cx notifications connectors list` | List notification connectors |
| `cx notifications connectors get <id>` | Get connector details |
| `cx notifications connectors create --from-file` | Create a connector |
| `cx notifications connectors update --from-file` | Update a connector |
| `cx notifications connectors delete <id>` | Delete a connector |
| `cx notifications connectors types` | List connector types |
| `cx notifications connectors entity-types` | List entity types |
| `cx notifications connectors entity-subtypes --type <type>` | List entity subtypes |
| `cx notifications routers list` | List notification routers |
| `cx notifications routers get <id>` | Get router details |
| `cx notifications routers create --from-file` | Create a router |
| `cx notifications routers update --from-file` | Update a router |
| `cx notifications routers delete <id>` | Delete a router |
| `cx notifications routers validate-matcher --from-file` | Test entity label matcher |
| `cx notifications presets list` | List notification presets |
| `cx notifications presets get <id>` | Get preset details |
| `cx notifications presets create --from-file` | Create a custom preset |
| `cx notifications presets update --from-file` | Update a custom preset |
| `cx notifications presets delete <id>` | Delete a custom preset |
| `cx notifications presets set-default <id>` | Set default preset |
| `cx notifications test connector --from-file` | Test connector config |
| `cx notifications test destination --from-file` | Test destination |
| `cx notifications test preset --from-file` | Test preset config |
| `cx notifications test routing-condition --from-file` | Test routing condition |
| `cx notifications test template-render --from-file` | Test template rendering |

### Integrations

| Command | Purpose |
|---|---|
| `cx integrations list` | List all integrations |
| `cx integrations get <id>` | Get integration details |
| `cx integrations create --from-file` | Create an integration |
| `cx integrations update <id> --from-file` | Update an integration |
| `cx integrations delete <id>` | Delete an integration |
| `cx integrations test --from-file` | Test integration config |
| `cx integrations template` | Get integration template |
| `cx integrations definition <id>` | Get integration definition |
| `cx integrations deployed <id>` | Get deployed integration |
| `cx integrations extensions list` | List available extensions |
| `cx integrations extensions get <id>` | Get extension details |
| `cx integrations extensions deployed` | List deployed extensions |
| `cx integrations extensions deploy --from-file` | Deploy an extension |
| `cx integrations extensions update --from-file` | Update deployed extension |
| `cx integrations extensions undeploy --from-file` | Undeploy an extension |
| `cx integrations contextual-data list` | List contextual data integrations |
| `cx integrations contextual-data get <id>` | Get contextual data details |
| `cx integrations contextual-data create --from-file` | Create contextual data integration |
| `cx integrations contextual-data update <id> --from-file` | Update contextual data integration |
| `cx integrations contextual-data delete <id>` | Delete contextual data integration |
| `cx integrations contextual-data definition <id>` | Get contextual data definition |
| `cx integrations contextual-data test <id>` | Test contextual data integration |

All commands support `-o json` for structured output and `-p <profile>` for profile selection.

---

## New Service Setup Workflow

When onboarding a new service, follow this checklist:

### 1. Create Saved Views

Set up views for the service's key log queries:

```bash
cx views folders create --from-file folder.json
cx views create --from-file view.json
```

### 2. Set Up Notification Connectors

Configure channels (Slack, PagerDuty, email):

```bash
cx notifications connectors types -o json
cx notifications connectors create --from-file slack-connector.json
```

### 3. Configure Notification Routing

Route alerts to the right channels:

```bash
cx notifications routers create --from-file router.json
```

### 4. Set Up Webhooks

Configure outgoing webhooks for external integrations:

```bash
cx webhooks types -o json
cx webhooks create --from-file webhook.json
cx webhooks test <webhook-id>
```

### 5. Install Integrations

Deploy relevant integrations and extensions:

```bash
cx integrations list -o json
cx integrations create --from-file integration.json
cx integrations extensions deploy --from-file extension.json
```

### 6. Create Dashboard

Use the `cx-dashboards` skill for the full dashboard creation workflow.

### 7. Create SLOs

Use the `cx-incident-management` skill for SLO creation and monitoring.

---

## Notification Setup Workflow

Detailed notification channel configuration:

### 1. List Available Connector Types

```bash
cx notifications connectors types -o json
```

### 2. Create a Connector

```bash
cx notifications connectors create --from-file connector.json
```

### 3. Create a Router

```bash
cx notifications routers create --from-file router.json
```

### 4. Assign or Create a Preset

```bash
cx notifications presets list -o json
cx notifications presets create --from-file preset.json
cx notifications presets set-default <preset-id>
```

### 5. Test End-to-End

```bash
cx notifications test connector --from-file test-connector.json
cx notifications test destination --from-file test-destination.json
cx notifications test routing-condition --from-file test-condition.json
```

---

## Webhook Setup

### 1. List Webhook Types

```bash
cx webhooks types -o json
```

### 2. Create Webhook

Template from an existing webhook if possible:

```bash
cx webhooks get <existing-id> -o json > webhook-template.json
cx webhooks create --from-file webhook.json
```

### 3. Test Webhook

```bash
cx webhooks test <webhook-id>
```

### 4. Create Automation Actions (Optional)

```bash
cx webhooks actions create --from-file action.json
cx webhooks actions reorder --from-file order.json
```

---

## Key Principles

- **Always test after setup** - use `cx notifications test`, `cx webhooks test`, `cx integrations test`
- **Use `--from-file`** for complex JSON payloads - pipe from stdin or use a file
- **Template from existing** - `cx <command> get <id> -o json > template.json` before creating
- **Check connector types first** - `cx notifications connectors types` and `cx webhooks types` before creating

---

## Related Skills

- **`cx-dashboards`** - dashboard creation and replace workflow
- **`cx-incident-management`** - SLO creation and alert triage workflows
- **`cx-alerts`** - alert definitions that trigger notifications
- **`cx-telemetry-querying`** - verify data flows after setup
