# Manual test plan: Fleet configuration groups and overlays

This plan exercises the new `cx configuration-groups` and `cx configuration-overlays` commands against a non-production Coralogix profile. It creates disposable resources in an inactive state, checks list/get/create/update/archive behavior, and never activates a family or overlay.

## Before you start

Use a staging or other disposable account with Fleet Manager access. Choose a local cx profile that points to that account; do not use a production profile. The examples below require `jq` only to copy IDs from command output. Replace the profile name and resource-name suffix with your own values.

From the cx-cli worktree, build and select the profile:

```sh
cd /Users/povilas.versockas/.codex/worktrees/cx-cli/six-cli-config-management
cargo build --locked
PROFILE=staging
```

## 1. Check list and pagination

List one page of groups and overlays in JSON:

```sh
cargo run -- --profile "$PROFILE" -o json configuration-groups list --page-size 2
cargo run -- --profile "$PROFILE" -o json configuration-overlays list --page-size 2
```

If either response contains `next_page_tokens`, run the same command again with that token as `--page-token`. Reuse the same `--page-size` and `--include-archived` options as the first request. Also run one list command without `-o json` to check the text table.

## 2. Create and update an inactive configuration group

Create `/tmp/cx-cli-manual-group.json`, replacing the name suffix so it is unique:

```json
{
  "name": "cx-cli-manual-group-REPLACE-ME",
  "description": "Temporary inactive group for cx CLI testing",
  "family": {
    "active": false,
    "raw": {
      "remoteConfigurations": [
        {
          "name": "manual-test",
          "rawConfiguration": "receivers:\n  otlp:\n    protocols:\n      grpc: {}\n      http: {}\nexporters:\n  debug: {}\nservice:\n  pipelines:\n    traces:\n      receivers: [otlp]\n      exporters: [debug]\n"
        }
      ]
    }
  }
}
```

Create it, then copy the returned `id`:

```sh
cargo run -- --profile "$PROFILE" --yes -o json configuration-groups create --from-file /tmp/cx-cli-manual-group.json
GROUP_ID='paste-returned-id-here'
```

Verify it appears in `configuration-groups list`, and inspect the full nested configuration:

```sh
cargo run -- --profile "$PROFILE" -o json configuration-groups get "$GROUP_ID"
```

Create `/tmp/cx-cli-manual-group-update.json` containing `{"description":"Updated by cx CLI manual test"}`, then exercise an update from stdin:

```sh
cat /tmp/cx-cli-manual-group-update.json | cargo run -- --profile "$PROFILE" --yes -o json configuration-groups update "$GROUP_ID" --update-mask description --from-file -
```

Get the group again and confirm the description changed while `family.active` is still `false`.

## 3. Create and update an inactive overlay

Create `/tmp/cx-cli-manual-overlay.json`, replacing the name suffix so it is unique:

```json
{
  "name": "cx-cli-manual-overlay-REPLACE-ME",
  "description": "Temporary inactive overlay for cx CLI testing",
  "active": false,
  "custom": {
    "configuration": "service:\n  telemetry:\n    logs:\n      level: debug\n",
    "targets": []
  }
}
```

Create it and copy the returned `id`:

```sh
cargo run -- --profile "$PROFILE" --yes -o json configuration-overlays create --from-file /tmp/cx-cli-manual-overlay.json
OVERLAY_ID='paste-returned-id-here'
```

Verify it appears in `configuration-overlays list`, then check its complete body:

```sh
cargo run -- --profile "$PROFILE" -o json configuration-overlays get "$OVERLAY_ID"
```

Create `/tmp/cx-cli-manual-overlay-update.json` containing `{"description":"Updated by cx CLI manual test"}`, then update it:

```sh
cargo run -- --profile "$PROFILE" --yes -o json configuration-overlays update "$OVERLAY_ID" --update-mask description --from-file /tmp/cx-cli-manual-overlay-update.json
```

Get it again and confirm the description changed while `active` remains `false` and `custom.targets` remains empty.

## 4. Archive and verify archived-resource reads

Archive each disposable resource:

```sh
cargo run -- --profile "$PROFILE" --yes configuration-overlays archive "$OVERLAY_ID"
cargo run -- --profile "$PROFILE" --yes configuration-groups archive "$GROUP_ID"
```

Confirm they no longer appear in the default lists. Then retrieve each with `--include-archived` and confirm the response has `archived: true`:

```sh
cargo run -- --profile "$PROFILE" -o json configuration-overlays get "$OVERLAY_ID" --include-archived
cargo run -- --profile "$PROFILE" -o json configuration-groups get "$GROUP_ID" --include-archived
cargo run -- --profile "$PROFILE" -o json configuration-overlays list --include-archived --page-size 2
cargo run -- --profile "$PROFILE" -o json configuration-groups list --include-archived --page-size 2
```

## What this walkthrough does not test

The group family and overlay both remain inactive, so this verifies CLI/API management flows without changing configurations delivered to collectors. Overlay target matching, activation, scheduled activation, and agent application require an active test configuration and should be tested only in a disposable environment with an explicit target. Automated E2E lifecycle coverage is in `tests/e2e/configuration_management.rs`; this manual walkthrough gives you the same safe staging scenario to run interactively.

When finished, remove the temporary JSON files:

```sh
rm -f /tmp/cx-cli-manual-group.json /tmp/cx-cli-manual-group-update.json /tmp/cx-cli-manual-overlay.json /tmp/cx-cli-manual-overlay-update.json
```
