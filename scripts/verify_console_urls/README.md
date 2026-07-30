# verify_console_urls.py

A development/verification aid for the "View in Coralogix" console-link
feature (`src/console_url.rs`). It is **not** part of the shipped `cx`
product, and is never invoked by `cx` itself, CI, or any release process -
it's a script a developer runs by hand, once, against a real Coralogix test
team, to sanity-check that the URLs `cx` prints actually resolve to the right
place in the real web console.

## Why this exists, and why it isn't a `cargo test`

`cx` prints a `View in Coralogix: <url>` line on stderr - and, for `-o json`
/`-o agents`, embeds the same URL as a `consoleUrl` field - after most
mutation/read commands. That logic is thoroughly unit- and wiremock-tested
(see `tests/console_urls/main.rs` and the doc-tests in `src/console_url.rs`),
but those tests only check that `cx` builds the URL it *intends* to build.
They can't catch:

- A route that has moved or been renamed in the real web console.
- A systematically wrong host or region-to-domain mapping.
- A real team whose `/identity/whoami` response doesn't have the shape
  `src/identity.rs` expects (e.g. missing `team_url`).
- Any other case where the *intended* URL and the *correct* URL have quietly
  diverged.

The only way to catch those is to actually run `cx` against a real team and
open (or otherwise verify) the URL it prints. That's what this script
automates, as much as it safely can without a browser in the loop: it drives
the real `cx` binary, captures the URL from both stderr and `-o json` output,
and asserts it has the expected shape (right domain, right hash-route, right
entity ID). It does **not** load the URL in a browser or check that the page
renders - only that the URL's shape is right. A human should still spot-check
a handful of the printed URLs in an actual browser once in a while.

## ⚠️ This script creates and deletes real objects on a real team

Several checks create a throwaway object (e.g. a dashboard, an alert) on
whatever team/profile you point it at, verify the console link, and then
delete the object again. Every created object is named
`console-url-check (safe to delete)` (plus a random suffix) so it's
unambiguous if cleanup ever fails and something is left behind. **Do not run
this against a production team you care about** - use a disposable test/dev
team. If a cleanup step fails, the script prints a `WARNING` (it does not fail
the run) telling you exactly what to delete by hand.

The `cases` checks are read-only by default (`cx cases get`) and only mutate
a case if you explicitly pass `--case-mutate` (see below).

## Prerequisites

1. Build `cx` from the repo root:

   ```bash
   cargo build --bin cx
   ```

   (the script will find `./target/debug/cx` automatically; see `--cx-bin`
   below if you built a release binary or have `cx` elsewhere.)

2. Credentials for a real, disposable Coralogix test team, via any of the
   mechanisms `cx` itself supports (see `docs/configuration.md`):

   - Environment variables: `CX_API_KEY` and `CX_REGION` (e.g. `eu2`).
   - A profile in `~/.cx/profiles/<name>.toml`, selected with `--profile
     <name>` (see `--profile` below).
   - The default profile in `~/.cx/config.toml`, if you have one set up.

No credentials are bundled with or read from this repo - you must supply
your own test team.

## Quick start

```bash
export CX_API_KEY=...      # API key for a disposable test team
export CX_REGION=eu2       # whichever region that team is on
python3 scripts/verify_console_urls/verify_console_urls.py
```

This runs every check it safely can with zero extra setup: all 16
static/page-level checks, and the 6 entity checks whose fixtures are already
checked into `fixtures/` (dashboards, alerts, e2m, iam-roles, iam-scopes,
parsing-rules). It skips (does not fail) the entity checks whose fixture
files aren't provided, `olly`, and `cases` - see below for how to fill those
in.

Read the full output top to bottom; it ends with a summary line and the
process exits non-zero if anything actually `FAIL`ed or `ERROR`ed (a `SKIP`
does not affect the exit code).

## Command-line options

| Flag | Meaning |
|---|---|
| `--profile <name>` | Passed straight through as `cx -p <name>`. Omit to use `CX_API_KEY`/`CX_REGION` env vars or your default `~/.cx` profile. |
| `--cx-bin <path>` | Path to the `cx` binary. Default: `cx` on `PATH`, then `./target/debug/cx`, then `./target/release/cx` (resolved relative to the repo root). |
| `--fixtures-dir <dir>` | Directory to read entity-creation fixtures from. Default: `scripts/verify_console_urls/fixtures/` (next to this script). |
| `--case-id <id>` | An existing case ID (UUID or `CASE-123`-style readable ID) on your test team. Required to run the `cases` check at all - see "Cases" below. |
| `--case-mutate` | Also exercise a real mutating case command against `--case-id` (see "Cases" below). Off by default. |
| `--include-olly` | Also check `cx olly ask`'s console link. Off by default - unlike every other check, it triggers a real AI-assistant call (real latency, real cost, non-deterministic response), not a cheap list/get. |
| `--only <a,b,c>` | Comma-separated list of check names to run (see the tables below for the exact names). Everything else is skipped. Useful while iterating on one entity's fixture. |
| `--keep` | Don't delete entities this script creates. Useful when debugging a failure by hand (inspect the object in the console before it's cleaned up) - you become responsible for deleting it afterwards. |

## What gets checked

### Static (page-level) checks - no object creation needed

These 16 commands' console links point at a page that's the same regardless
of which item you're looking at (e.g. "the usage page", not "this specific
API key"), so a single safe read-only `list`/`get`/`summary` call is enough
to verify the link - nothing is created.

| Check name | Command run | Expected URL fragment |
|---|---|---|
| `usage` | `usage summary` | `/#/settings/datausage` |
| `tco` | `tco list` | `/#/tco-policies` |
| `archive-logs` | `archive logs get` | `/#/physical-locations` |
| `archive-metrics` | `archive metrics get` | `/#/physical-locations` |
| `recording-rules` | `recording-rules list` | `/#/recording-rules` |
| `enrichments` | `enrichments list` | `/#/enrichments` |
| `enrichments-custom-tables` | `enrichments custom list` | `/#/enrichments` |
| `integrations` | `integrations list` | `/#/extensions/integrations` |
| `integrations-extensions` | `integrations extensions list` | `/#/extensions/integrations` |
| `integrations-contextual-data` | `integrations contextual-data list` | `/#/extensions/integrations` |
| `webhooks` | `webhooks list` | `/#/extensions/outbound-webhooks` |
| `iam-api-keys` | `iam api-keys list` | `/#/settings/api-keys` |
| `iam-users` | `iam users search` | `/#/settings/team/members` |
| `iam-ip-access` | `iam ip-access get` | `/#/settings/login-access-policies` |
| `ai-center-applications` | `ai-center applications list` | `/#/ai-center/overview/application-catalog` |
| `ai-center-evaluations` | `ai-center evaluations list` | `/#/ai-center/overview/eval-catalog` |

Plus `olly` (`olly ask <message>`, expected fragment `/#/olly`) - **skipped
unless you pass `--include-olly`**, since it's the one "read-only" check that
isn't actually free: it's a real AI-assistant call.

### Entity checks - create, verify, delete

These 12 commands' console links are per-entity (`/#/dashboards/{id}`,
`/#/alerts/{id}`, etc.), so the script has to create a throwaway object to
get an ID to check the link against. For each one it:

1. Loads a JSON fixture from `fixtures/<file>.json` (or `--fixtures-dir`).
2. Copies it and appends a random suffix to its `name`/`displayName` so
   repeated runs don't collide on uniqueness constraints.
3. Runs `cx <command> create --from-file <tmp> --yes -o json`.
4. Extracts the new entity's ID from the response and checks both the
   `View in Coralogix:` stderr line and the response's `consoleUrl` field
   against the expected URL shape.
5. Runs `cx <command> delete <id> --yes` to clean up (always, even on
   failure - unless you passed `--keep`).

Each entity check has a **confidence tier**, reflecting how sure this repo
can be that the checked-in fixture is actually valid API payload (as opposed
to merely well-formed JSON):

| Confidence | Meaning |
|---|---|
| `confirmed` | The fixture is a real, working example payload cited in this repo's own skill docs. If `create` fails, that's a real bug (in the fixture, the create endpoint, or the console-link code). |
| `best-effort` | The fixture was inferred from this repo's Rust struct field names, since no worked example exists in-repo. It's plausible but unverified - if `create` fails, it might just mean the fixture needs adjusting, not that anything is actually broken. The script prints an explicit note to that effect. |
| `requires-fixture` | The entity's request schema isn't documented anywhere in this repo (this codebase's own skill docs explicitly recommend templating from a live object rather than hand-authoring one - see per-entity instructions below). No fixture is checked in; the check **skips** until you supply one yourself. |

| Check name | Confidence | Fixture file | Expected URL fragment |
|---|---|---|---|
| `alerts` | confirmed | `alert.json` | `/#/alerts/{id}` |
| `dashboards` | confirmed | `dashboard.json` | `/#/dashboards/{id}` |
| `e2m` | confirmed | `e2m.json` | `/#/tco/metrics/{id}` |
| `iam-roles` | best-effort | `role.json` | `/#/settings/roles?selectedRoleId={id}` |
| `iam-scopes` | best-effort | `scope.json` | `/#/settings/scopes?selectedScopeId={id}` |
| `parsing-rules` | best-effort | `parsing_rule.json` | `/#/rules/group/{id}` |
| `views` | requires-fixture | `view.json` | `/#/explore?viewId={id}` |
| `slos` | requires-fixture | `slo.json` | `/#/slo/{id}/overview` |
| `suppression-rules` | requires-fixture | `suppression_rule.json` | `/#/suppression-rules?edit={id}` |
| `notification-connectors` | requires-fixture | `connector.json` | `/#/notification-center/connectors?id={id}` |
| `notification-routers` | requires-fixture | `router.json` | `/#/notification-center/routers?id={id}` |
| `iam-groups` | requires-fixture | `group.json` | `/#/settings/account/groups?selectedGroupId={id}` |

#### Filling in a `requires-fixture` entity

For these 6 entities, running the script without a fixture prints a `SKIP`
with instructions specific to that entity. The general recipe is the same
for all of them: create one by hand in the real web console (however you'd
normally do it), then dump it back out with `cx` and drop it into
`fixtures/`:

```bash
cx views list -o json                                    # find the id
cx views get <id> -o json > scripts/verify_console_urls/fixtures/view.json
```

Repeat for `slos`, `alerts suppression-rules`, `notifications connectors`,
`notifications routers`, and `iam groups` (see the exact `get` command for
each in the script's `FIXTURE_INSTRUCTIONS`, or the `SKIP` message it prints).
Give the object a name you'll recognize (e.g. containing
`console-url-check`) so it's clear that any object with that name showing up
in `create`'s output is expected. You don't need to hand-edit anything else
in the dumped JSON - `create --from-file` accepts the same shape `get`
returns for every entity in this repo, and the script already
auto-uniquifies the name field before creating.

### Cases - you must supply an existing case ID

Cases are system-generated (from alerts/incidents) and **cannot be created
by `cx`**, so this script can't create-and-delete one the way it does for
dashboards, alerts, etc. Without `--case-id`, the `cases` check is skipped
with a message saying so.

Pass an existing case ID from your test team:

```bash
python3 scripts/verify_console_urls/verify_console_urls.py --case-id CASE-123
```

By default this only runs `cx cases get <id>` (fully read-only) and checks
its console link. If you also want to exercise a *mutating* case command
(closer to how most other checks in this script work), pass
`--case-mutate`, which additionally runs `cx cases set-priority <id>
--priority P5` and then `cx cases clear-priority <id>` to undo it. This
pair was chosen deliberately over `assign`/`resolve`/`close`/`update`: it's
purely cosmetic metadata, doesn't touch the case's status, assignee, or
content, and is trivially and losslessly reversible.

```bash
python3 scripts/verify_console_urls/verify_console_urls.py --case-id CASE-123 --case-mutate
```

## Limitations

- **No live credentials are available in the environment this script was
  authored in**, so it has been exercised for control flow (argument
  parsing, filtering, the "no fixture found" skip path, syntax) but **not
  end-to-end against a real Coralogix team**. Please run it against a real
  disposable test team before relying on it, and report back if anything
  doesn't match this README.
- The `best-effort` and `requires-fixture` tiers exist precisely because this
  repo doesn't have full documentation for every request schema. A failure
  on one of those checks doesn't necessarily mean the console-link feature
  is broken - read the printed detail, which calls this out explicitly.
- This script checks URL *shape* (domain + hash route + ID), not that the
  URL actually renders the right page in a browser. Do a manual spot check
  in a browser occasionally, especially after changing anything in
  `src/console_url.rs`.
