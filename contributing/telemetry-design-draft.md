# cx CLI telemetry — draft

Status: draft. The CLI side (section 1) is implemented in `src/telemetry/`. The
collector config (section 2) passes `otelcol validate` and a local end-to-end run
against the custom collector image built from `scripts/telemetry-collector/`: see
`scripts/telemetry-collector/config.yaml` and
`scripts/telemetry-collector-check.sh`. Sections 3 and 4 are not done. Section 5
holds the decisions from the 2026-11-16 design review. They are implemented, and
where they differ from sections 1 to 4, section 5 wins.

```
cx command → local spool → detached sender → HTTPS POST (OTLP/HTTP JSON)
           → API gateway (open path)
           → dedicated OTel collector (rate limit, validate, relabel) → c4c
```

## 1. Event schema

Each event is one OTLP log record. Every value is an enum, a UUID, a number, a
boolean, or a string matched by a strict pattern. There is no free text, and the log
body is empty.

### Resource attributes (same for every event in a request)

| Attribute | Type | Allowed values | Source in the CLI |
|---|---|---|---|
| `service.name` | string | `cx-cli` (the collector overwrites it) | constant |
| `service.version` | string | `MAJOR.MINOR.PATCH` | `CARGO_PKG_VERSION` |
| `os.type` | string | `darwin`, `linux`, `windows` | `std::env::consts::OS` |
| `host.arch` | string | `arm64`, `amd64` | `std::env::consts::ARCH` |
| `cx.install.id` | string | UUID v4 | `version_cache::installation_id()` |
| `cx.install.method` | string | `brew`, `curl`, `unknown` | `install_method::detect()` |

### Attributes on every event

| Attribute | Type | Allowed values |
|---|---|---|
| `event.name` | string | see the event list below |
| `cx.invocation.id` | string | UUID v4, one per process run |
| `cx.agent` | string | `direct`, or a lowercased name from `safety::AGENT_ENV_VARS` |
| `cx.ci` | bool | |
| `cx.tty` | bool | |
| `cx.team.id` | string, optional | digits only. Claimed by the client, not verified |

The record timestamp is the time the event happened on the user's machine.

### Events

| `event.name` | When | Extra attributes |
|---|---|---|
| `cli.first_run` | `~/.cx/state.json` is created | none |
| `cli.command` | a command finishes | command set below |
| `cli.parse_error` | clap rejects the arguments | `cx.parse.kind`: `unknown_command`, `unknown_flag`, `missing_arg`, `other` |
| `cli.onboarding` | an onboarding step happens | `cx.onboarding.step`: `init_started`, `init_completed`, `profile_added`, `oauth_started`, `oauth_succeeded`, `oauth_failed`, `first_query_succeeded`; `cx.auth.method`: `oauth`, `api_key`; `cx.credential.store`: `keychain`, `file` |
| `cli.skills_install` | `cx skills install` finishes | `cx.skills.scope`: `global`, `local`; `cx.outcome` |
| `cli.update_notice` | an update notice is printed | `cx.version.latest` |
| `cli.version_changed` | first run after an upgrade | `cx.version.previous` |
| `cli.confirm` | a risky-command prompt is resolved | `cx.confirm.result`: `accepted`, `declined`, `skipped_yes`, `non_interactive` |

### Command set (for `cli.command`)

| Attribute | Type | Allowed values |
|---|---|---|
| `cx.command.path` | string | subcommand names joined by spaces, e.g. `alerts list`. Max 4 words |
| `cx.command.flags` | string | sorted flag names joined by commas, e.g. `--limit,--start`. Names only |
| `cx.outcome` | string | `success`, `error`, `cancelled` |
| `cx.error.class` | string | `none`, `auth`, `permission`, `api`, `timeout`, `http`, `json`, `io`, `query_stream`, `usage` |
| `cx.http.status_class` | string | `none`, `2xx`, `4xx`, `5xx` |
| `cx.duration_ms` | int | 0 to 3,600,000 |
| `cx.output.format` | string | `text`, `json`, `toon` |
| `cx.profile.count` | int | 0 to 50 |
| `cx.spilled` | bool | |

`cx.command.flags` is a joined string, not an array, so the collector can check
it with one pattern.

### Example request

`POST https://<host>/api/v2/cli-telemetry`, `Content-Type: application/json`

```json
{
  "resourceLogs": [{
    "resource": { "attributes": [
      { "key": "service.name", "value": { "stringValue": "cx-cli" } },
      { "key": "service.version", "value": { "stringValue": "0.1.32" } },
      { "key": "os.type", "value": { "stringValue": "darwin" } },
      { "key": "host.arch", "value": { "stringValue": "arm64" } },
      { "key": "cx.install.id", "value": { "stringValue": "3f0e6c1a-9b7d-4c55-8e21-5a6f0d2b7c90" } },
      { "key": "cx.install.method", "value": { "stringValue": "brew" } }
    ]},
    "scopeLogs": [{ "logRecords": [{
      "timeUnixNano": "1794480000000000000",
      "attributes": [
        { "key": "event.name", "value": { "stringValue": "cli.command" } },
        { "key": "cx.invocation.id", "value": { "stringValue": "b1d1c0de-2f43-4a0b-9d0e-6c1f5e7a8b90" } },
        { "key": "cx.agent", "value": { "stringValue": "claude_code" } },
        { "key": "cx.ci", "value": { "boolValue": false } },
        { "key": "cx.tty", "value": { "boolValue": false } },
        { "key": "cx.command.path", "value": { "stringValue": "logs" } },
        { "key": "cx.command.flags", "value": { "stringValue": "--limit,--start" } },
        { "key": "cx.outcome", "value": { "stringValue": "success" } },
        { "key": "cx.error.class", "value": { "stringValue": "none" } },
        { "key": "cx.http.status_class", "value": { "stringValue": "2xx" } },
        { "key": "cx.duration_ms", "value": { "intValue": "412" } },
        { "key": "cx.output.format", "value": { "stringValue": "toon" } },
        { "key": "cx.profile.count", "value": { "intValue": "1" } },
        { "key": "cx.spilled", "value": { "boolValue": false } }
      ]
    }]}]
  }]
}
```

### Client rules

- The CLI builds the JSON with `serde_json` and sends it with `reqwest`. No OTel SDK.
- HTTP 429 and 503 keep the batch. Any other HTTP response means the batch is
  finished and removed from the spool.
- A network failure keeps the batch until it is older than 7 days.
- At most 50 events per request.
- No command sends. A detached sender sub-process does, after the command has
  exited (section 5.1).

## 2. Collector config

A separate collector deployment, used only for CLI events.

The config is `scripts/telemetry-collector/config.yaml`.

What the collector does not do, and where it is handled:

| Gap | Handled by |
|---|---|
| Per-IP rate limit | a `ratelimit` processor in a custom collector image (section 5.5) |
| Duplicate events | deduplicate on `cx.invocation.id` + `event.name` in c4c queries |
| "This flag belongs to this command" | not checked. Flag and command names are pattern-limited only |
| Verified `team_id` | not available on an open path. `cx.team.id` is client-claimed |
| Volume cap | daily quota on application `cx-cli` in c4c |

## 3. Deployment with platform features

There is no OTel collector platform feature (checked in pipeline-handler and the IDP
MCP). There is a precedent: the platform's own collectors are a `chart` deployable in
`coralogix/eng-observability` (`opentelemetry/`), owned by the Observability team.
They use the `helm` feature with the internal `otel-integration` chart (JFrog
`coralogix-charts-virtual`) and the internally mirrored `otelcontribcol` image.

Decision: deploy ours as a plain service (not the platform chart). It needs one
receiver, two processors and one exporter, so the cluster-collection chart is more
than we need.

The layout copies `coralogix/cx-cli-onboarding-service`, the existing cxai service
with a public route. Field names were checked against the live v2 schemas. The files
have not been rendered by pipeline-handler or applied.

```
cx-cli-telemetry-collector/          # new repo, or a folder in an existing cxai repo
├── meta.yaml
└── platform/defaults/
    ├── workloads/defaults.yaml
    ├── configMap/defaults.yaml
    ├── envVars/defaults.yaml
    └── networking/defaults.yaml
```

`meta.yaml`

```yaml
name: cx-cli-telemetry-collector
deployable_type: application        # to confirm: no code to build, image comes from workloads
lifecycle_status: development
namespace: ai
enabled: true
cx_metadata:
  cx_feature_id: ai
  cx_service_rnd_owner_id: cxai
auto_deploy:
  - cx10
deploy_envs:
  - cx10                            # staging first; the production cluster is an open question
platform_features_version: v2
```

`platform/defaults/workloads/defaults.yaml`

```yaml
# yaml-language-server: $schema=https://handler.platform.coralogix.net/docs/pf/v2/workloads/schema
kind: deployment
replicas: 2
container:
  image: '{CX_CONTAINER_REPOSITORY}/otelcontribcol:<pinned tag>'   # the mirrored image the platform collectors use
  args:
    - --config=/etc/otelcol/config.yaml
  resources:
    requests: { cpu: 100m, memory: 256Mi }
    limits: { cpu: 500m, memory: 512Mi }
  probes:
    liveness:
      httpGet: { path: /, port: 13133 }
    readiness:
      httpGet: { path: /, port: 13133 }
```

`platform/defaults/configMap/defaults.yaml`

```yaml
# yaml-language-server: $schema=https://handler.platform.coralogix.net/docs/pf/v2/configMap/schema
mountPath: /etc/otelcol
data:
  - fileName: config.yaml
    data: |
      # the collector config from section 2
```

`platform/defaults/envVars/defaults.yaml`

```yaml
# yaml-language-server: $schema=https://handler.platform.coralogix.net/docs/pf/v2/envVars/schema
environmentVariables:
  CORALOGIX_DOMAIN: <c4c domain for this environment>   # c4c-staging for cx10
  CORALOGIX_PRIVATE_KEY:
    secretKeyRef:
      key: CORALOGIX_PRIVATE_KEY      # name omitted: defaults to the service-level external secret
```

`platform/defaults/networking/defaults.yaml`

```yaml
# yaml-language-server: $schema=https://handler.platform.coralogix.net/docs/pf/v2/networking/schema
ingress:
  rateLimitDefinitions:
    - name: cli-telemetry
      unit: second
      requestsPerUnit: 100            # global ceiling; the per-IP limit is in the collector (5.5)
      shadowed: true                  # required field; start in shadow mode
  endpoints:
    - name: api-v2-cli-telemetry
      protocol: http
      path: /api/v2/cli-telemetry
      matchType: exact
      rewritePath: /v1/logs           # only the logs path of the collector is reachable
      port: 4318
      usage:
        type: mgmt
      timeout: 5
      retries: 0
      limitRef: cli-telemetry
      authentication:
        authTypes:
          - unauthenticated: {}
ports:
  - name: otlp-http
    port: 4318
    protocol: TCP
    targetPort: 4318
```

Differences from the onboarding service's route: `unauthenticated` instead of
`userToken`, an exact path match, no `signAuthContext`, and no retries. The
onboarding route also sets `legacyEndpointFeatures.migration: true`; whether a new
endpoint needs that is not known.

Before the first deploy:

- Create a c4c-staging send-your-data key and store it in the service's secret.
- Set a daily quota for application `cx-cli` in that c4c team.
- Run the config through `otelcol validate` with the pinned image.

## 4. Open questions

- Does the gateway forward `x-envoy-external-address` to the service, and is the
  route limit on an unauthenticated endpoint one shared bucket? (5.5)
- Which cluster hosts the collector, and what fixed hostname does the CLI use before
  the user has a region?
- Who owns the collector deployment?
- Is a client-claimed `team_id` acceptable, or is a second authenticated endpoint
  needed?
- Can an `application` deployable run with only `container.image` and no build? The
  only repo that ships a collector as its own deployable
  (`eng-onlineboutique-demo/otelcollector`) builds an image with the config copied in.
- Can the platform build and host the custom collector image from 5.5?
- `error_mode: propagate` on the validation processors is deliberate. The internal
  OTTL skill recommends `ignore` for production, but with `ignore` a check that
  errors keeps the record, which lets bad data through. Confirm with Observability.

## 5. Design review decisions (2026-11-16)

### 5.1 Sender sub-process

`cx` no longer sends events itself. It appends the event to the spool, starts a
detached sender and exits. No command waits on the network for telemetry. Local
commands send too, so users who are not signed in yet are counted.

**How the sender starts**

- The sender is `cx __telemetry-send`: the same binary, started from
  `std::env::current_exe()`.
- `main` checks `argv[1]` for that name before completions, clap and the runtime.
  It is not a clap subcommand, so it is absent from help, `cx schema` and shell
  completions, and it runs no update check and records no telemetry.
- The spawn is the one `update_check::spawn_skills_refresh` already uses on
  master: stdin, stdout and stderr are null, with its own process group on Unix
  and `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` on Windows. Both callers
  share one helper. (This branch must merge master first to get it.)
- The sender reads the consent mode and the endpoint itself, the same way `cx`
  does. An opt-out set after the spawn still stops it.

**When it starts**

- At the end of the run, right after `cli.command` is appended, so this run's
  event goes out with this sender.
- Also before clap exits on a parse error, which never reaches the end of `main`.
- Only in `On` mode. With any opt-out there is no spool and no sender.
- Starting it at the beginning of the run was rejected: the spawn costs the same
  either way, and this run's event would wait for the next run.

**One sender at a time**

- Lock: `~/.cx/telemetry-sender.lock`, an OS advisory lock taken with
  `std::fs::File::try_lock` (in std since Rust 1.89; the toolchain is 1.96.1).
- The sender takes the lock first and exits if it is held.
- The parent tries the lock before spawning. If it is held, it does not spawn.
  If it is free, it releases it and spawns.
- Two parents can both see a free lock and both spawn. The second sender finds
  the lock held and exits. That costs one short process, never a double send.
- The kernel frees the lock when the sender dies, so there is no stale-lock age
  rule. This is why the lock is not the age-based `RefreshLock` used for skills.
- If the lock cannot be taken for any other reason (for example a file system
  without lock support), nothing is sent.
- The `.sending` claim in `spool.rs` stays as it is. It still protects against
  an append during a send, and a sender that is killed leaves a `.sending` file
  that the next sender picks up after 60 s.
- The parent also skips the spawn when there is no spool file, so a run that
  recorded nothing (`cx --help`) starts no process.

**Backoff**

There is no backoff file today. The sender adds one: `~/.cx/telemetry-backoff`,
an empty file.

- The sender touches it when the batch is kept (see below) and deletes it after
  a batch is accepted.
- The parent does not spawn while the file is younger than 15 minutes.
- Result: with the collector unreachable, one sender per 15 minutes per machine.

**What ends a batch**

| Result | Batch | Backoff |
|---|---|---|
| HTTP 429 or 503 | kept | yes |
| Any other HTTP status | removed | no |
| Network error or timeout | kept | yes |

This changes the "any HTTP response" rule in section 1: with a per-IP rate limit
(5.5), a 429 means the collector did not look at the batch.

**Timeouts**

- Connect 5 s, whole request 10 s.
- The sender sends batches of 50 until the spool is empty: at most 10 requests,
  because the spool holds at most 500 events.
- Lifetime cap 30 s, enforced by a watchdog thread that exits the process. A DNS
  lookup runs on a blocking thread that a request timeout cannot stop. The cap
  stays below the 60 s after which a `.sending` file counts as abandoned.

**The runtime shutdown in `main`**

The bounded 100 ms `shutdown_timeout` was added only for the in-process flush.
With no flush it is removed, and `main` goes back to `#[tokio::main]` as on
master, with the wrapper logic (`begin`, `run`, `finish`, spawn) inside it.

**Removed from the command path**

`telemetry::flush`, the `network` gate on flushing, and `send::BUDGET`.
`Invocation.network` and `mark_network` stay, because `cx.http.status_class`
uses them: `cx init` and `cx profiles add` call the API without building
execution targets.

**CI, containers and sandboxes**

The sender is started in every environment, including when `CI` is set. There is
no special case.

| Environment | What happens | Checked |
|---|---|---|
| Parent exits at once | the sender keeps running | yes, macOS, Linux and Windows |
| `out=$(cx ...)` or a pipe | the caller does not wait for the sender, because it holds no stdio | yes, macOS |
| Container where `cx` is PID 1 | the kernel kills the sender when `cx` exits. The event stays in the spool. It is lost with the container unless `~/.cx` is a mount | yes, Linux |
| CI runner | the sender has until the job ends. GitHub Actions kills leftover processes at the end of the job | not checked |
| Sandbox with no network | network error, then backoff: one sender per 15 minutes | by design |
| Sandbox that forbids new processes | the spawn fails and the error is ignored. Events wait in the spool for up to 7 days | by design |

**Cost to the command**

Measured with a throwaway spike that spawns a detached child 200 times and times
the `spawn()` call in the parent.

| OS | Child binary | Median | p95 |
|---|---|---|---|
| macOS 26, Apple M4 Pro | `cx` release build, 10 MB | 0.4 ms | 0.8 ms |
| macOS 26, Apple M4 Pro | `cx` debug build, 99 MB | 1.4 ms | 2.1 ms |
| Linux 6.12 arm64 (Docker Desktop VM) | 100 MB binary on local disk | 0.2 ms | 0.6 ms |
| Linux 6.12 arm64 (Docker Desktop VM) | small binary on a bind mount | 1.7 ms | 2.9 ms |
| Linux x86_64, GitHub `ubuntu-latest` | `cx` 0.1.34 release, 16 MB | 0.1 ms | 0.1 ms |
| Windows x86_64, GitHub `windows-latest` | `cx` 0.1.34 release, 21 MB | 1.4 ms | 1.5 ms |

The parent's lock check (open, `try_lock`, close) costs 14 µs on macOS, 21 µs
on Windows and 1 to 3 µs on Linux.

The cost to a command is one spawn plus one lock check: 0.1 ms on Linux, about
0.5 ms on macOS and 1.4 ms on Windows with a release build, and under 3 ms at
p95 in every case measured. Before, it was up to 400 ms.

**As built** (macOS 26, Apple M4 Pro, release build, `cx dataprime list`, 40
runs each, wall time of the whole command). Before every run any sender was
killed and the backoff file removed, so every run paid for a spawn.

| Scenario | Median | p95 |
|---|---|---|
| Telemetry off | 10.4 ms | 10.8 ms |
| Collector refuses the connection | 10.8 ms | 11.1 ms |
| Collector accepts and never answers | 11.0 ms | 11.5 ms |
| DNS stalls (a `.local` name, 5 s mDNS timeout) | 10.9 ms | 11.7 ms |

Telemetry adds 0.4 to 0.6 ms at the median and under 1 ms at p95, whatever the
collector does. That includes building the event and appending it to the
spool. With the in-process flush, the DNS case took about 700 ms.

One outlier: the first spawn of the freshly downloaded `cx.exe` on Windows took
82 ms, once in 600 spawns. That is most likely the antivirus scan of a new
binary, so it can happen on the first run after an install or upgrade.

### 5.2 CLI and collector deployment dependency

The collector knows every event name, attribute and enum value. A CLI that
sends something the collector does not know has that record dropped, silently.
So the two must be released in order.

**The rule**

1. `scripts/telemetry-collector/config.yaml` in this repo is the only source of
   the collector config. The collector service carries a copy in its configMap
   and never edits it by hand.
2. Schema changes only add: a new event, a new attribute, a new enum value.
   Nothing is removed or narrowed, because old CLI versions keep sending the old
   shape for as long as they are installed.
3. The collector is deployed first. A CLI release that needs a config change
   ships only after that config is live.

**Where it is enforced**

| Place | Check | Catches |
|---|---|---|
| `test.yml`, new `telemetry-contract` job on every PR | runs `scripts/telemetry-collector-check.sh` with the config in the PR | a CLI change that adds an event or value without updating the config |
| `release.yml`, new step before the build matrix | fetches the config copy from the collector service repo's default branch and runs the same script with it | a release cut before the collector has the new config |

Both run on `ubuntu-latest`, which has Docker. Both use a debug build of `cx`,
because the script reads events through the Log mode (5.4). The script takes
the config to check from `CONFIG`.

The release job is skipped until the collector service repo exists. M6 turns
it on, together with `CX_TELEMETRY_ENDPOINT`, by setting the repository
variables `TELEMETRY_COLLECTOR_REPO` (`owner/name`) and
`TELEMETRY_COLLECTOR_CONFIG_PATH`, and the secret
`TELEMETRY_COLLECTOR_REPO_TOKEN`, a token that can read that repo.

The script learns what the CLI sends from its own scenario, which must cover
every event kind. A new event kind needs a line in the scenario, or the check
does not see it.

The release step compares against the service repo's default branch. That is
the deployed config only while the service auto-deploys from that branch, as
`auto_deploy` in section 3 sets up.

**Old and new versions**

| CLI | Collector | Result |
|---|---|---|
| old | new | accepted. The config only ever adds |
| new | old (the release gate was bypassed) | records with an unknown event name or enum value are dropped. Unknown attributes are stripped and the record is kept. The CLI sees a 200 either way and nothing breaks for the user |

### 5.3 `cx.team.id` is optional

`cx.team.id` is present only when the run used exactly one profile, with that
profile's own stored credentials, and the profile file already has
`cached_team_id`. That field is written after an OAuth sign-in or a successful
whoami lookup. So every event before sign-in, every local command, every
multi-profile run and every run with `CX_API_KEY` has no team id.

Audit of every place that touches it:

| Place | Assumes it is present? |
|---|---|
| Event model (`event.rs`): `Option<TeamId>`, skipped when `None`, defaults to `None` when read | no |
| OTLP encoder (`send.rs`): an absent key gives no attribute | no |
| Collector validation: `attributes["cx.team.id"] != nil and not IsMatch(...)` | no |
| Collector `keep_keys`: lists the key, keeps the record without it | no |
| Spec, section 1: "string, optional" | no |

Gaps (1 and 2 are fixed):

1. `scripts/telemetry-collector-check.sh` never sends an event that has a team
   id. Its profile is created with `--api-key` against a stub, so
   `cached_team_id` is never set. The script must write `cached_team_id` into
   the profile file for one run and assert that the collector exports those
   events with `cx.team.id`, next to the ones without it.
2. The unit test `team_id_is_a_string_and_one_record_per_event` checks the
   record that has a team id. It does not assert that the second record has no
   `cx.team.id` key. Add that assert.
3. Events recorded before `set_targets` in the same run (`cli.first_run`,
   `cli.version_changed`) never carry a team id, even for a signed-in user.
   This is accepted, not fixed.

Rules for the dashboards (none exist yet):

- Never filter on `cx.team.id` being present. Events without it go in an
  "unknown" group.
- Count users and installs by `cx.install.id`, not by team.
- The onboarding funnel is keyed on `cx.install.id`. To show a team for the
  early steps, join to the first later event from the same install that has a
  team id.
- Per-team numbers are lower bounds.

### 5.4 Log mode only in debug builds

**The flag:** `cfg!(debug_assertions)`. The Log mode exists in debug builds
(`cargo build`, `cargo run`, `cargo test`) and not in release builds
(`cargo build --release`, which is what `release.yml` ships).

A cargo feature was rejected. `cargo test` builds the binary in the dev
profile, so with `debug_assertions` the integration tests keep working with no
change to CI. A feature would need `--features` on every test command and
`required-features` on the test target.

**On a release build, `CX_TELEMETRY=log` means off.** Today an unknown value
does not opt out. Someone who set `log` to make sure nothing is sent must not
start sending because the mode was removed.

**Tests**

- `tests/telemetry/main.rs` keeps using `CX_TELEMETRY=log`. The log-mode tests
  get `#[cfg(debug_assertions)]`, so `cargo test --release` skips them and does
  not fail.
- One new unit test on `consent::decide` covers both builds: `log` gives `Log`
  with debug assertions and `Off` without.
- `scripts/telemetry-collector-check.sh` needs a debug binary. It exits with a
  clear message if the binary ignores the Log mode.

**For users and reviewers on a release build**

1. `docs/telemetry.md` (M5) lists every field and every value.
2. To see the real bytes, point the endpoint at a local listener. The runtime
   `CX_TELEMETRY_ENDPOINT` already overrides the built-in one:

   ```bash
   nc -l 4318
   ```

   ```bash
   CX_TELEMETRY_ENDPOINT=http://localhost:4318/v1/logs cx logs --limit 1
   ```

   The listener prints the exact request the collector would get, and nothing
   reaches Coralogix. `docs/telemetry.md` documents this in place of
   `CX_TELEMETRY=log`.

No new command is added for this.

### 5.5 Collector rate limit, per source IP

**Mechanism:** Elastic's `ratelimit` processor
(`github.com/elastic/opentelemetry-collector-components/processor/ratelimitprocessor`
v0.81.0, built against collector v0.165.0), first in the pipeline after
`memory_limiter`.

The stock `otel/opentelemetry-collector-contrib:0.165.0` image has no rate
limiter. `otelcol components` lists none, and the config below fails `validate`
on it with `unknown type: "ratelimit"`. So the collector becomes a custom image
built with `ocb`. This answers two open questions in section 4: the deployable
does build an image, and the tag to pin is ours.

The image holds only what the config uses: the `otlp` receiver; the
`memory_limiter`, `batch`, `filter`, `transform` and `ratelimit` processors; the
`coralogix` exporter (plus `file` and `debug` for the local check); the
`health_check` extension; the `env` and `file` config providers. It is 61 MB,
against 484 MB for contrib. The `ocb` manifest (`builder.yaml`) and the
Dockerfile are in `scripts/telemetry-collector/`, and the check script builds
and uses this image.

**Config** (changes to `scripts/telemetry-collector/config.yaml`):

```yaml
receivers:
  otlp:
    protocols:
      http:
        include_metadata: true    # request headers become client metadata

processors:
  ratelimit/ip:
    metadata_keys: [x-envoy-external-address]
    strategy: requests
    rate: 10                      # per second, per key
    burst: 300
    throttle_behavior: error      # reject with HTTP 429

service:
  pipelines:
    logs:
      processors: [memory_limiter, ratelimit/ip, filter/validate, transform/sanitize, batch]
```

With these changes the full config passes `validate` on the custom image, with
and without `local.yaml`.

**The client IP**

- The peer address the collector sees is the gateway, so it cannot be the key.
- The key is the `x-envoy-external-address` header, which the edge Envoy sets
  to the client address it trusts.
- `x-forwarded-for` must not be the key. The processor uses the whole header
  value, and Envoy appends to whatever the client sent. A client that sends its
  own `X-Forwarded-For` gets a new bucket on every request. Tested: keyed on
  XFF, 400 requests with client-chosen prefixes all passed.
- Requests without the header share one bucket.
- Not checked: that the Coralogix gateway forwards `x-envoy-external-address`
  to the service. M6 captures the real headers on cx10 before the limit is
  relied on. If the header is missing, only the `metadata_keys` line changes.

**The numbers**

- 10 requests per second per IP, sustained. One install sends about one request
  per command. A few hundred users behind one NAT, each running a command every
  30 to 60 s, stay under it.
- Burst 300, refilled in 30 s. This covers a CI fan-out or an agent burst.
- Worst case from one IP: 10 requests × 50 records = 500 records per second per
  replica. The daily quota on application `cx-cli` in c4c is the backstop.
- Buckets are per replica, so with 2 replicas the real limit is up to twice
  these numbers.

**What a limited client gets:** HTTP 429 with `Retry-After: 1`. Nothing from
that request is exported. The sender keeps the batch and backs off for 15
minutes (5.1).

`scripts/telemetry-collector-check.sh` runs a burst on every PR: 400 requests
from one address must get at least one 429, 5 from a second address must all
pass, and the number of records exported must equal the number of requests
accepted.

**Burst test** (custom image, local, 400 requests in 0.5 s):

| Step | Result |
|---|---|
| 400 requests from `203.0.113.7` | 304 × 200, 96 × 429 |
| 5 requests from `198.51.100.9` right after | 5 × 200 |
| 5 more from `203.0.113.7` | 5 × 429 |
| Records exported | 314, so a rejected request exports nothing |

**Known limits of the processor**

- Its stability level is `development`.
- Buckets are never evicted, so memory grows with the number of distinct IPs.
  Not measured. `memory_limiter` and the pod memory limit bound it; a restart
  clears it.
- It logs one error line per rejected request.

**With the gateway's route limit**

- The v2 `networking` schema has no per-IP scope. `RateLimit` has only
  `userLevelScope` (`userId`, `userAccountId` or null). So the gateway cannot
  limit per source IP, and the collector does it.
- On a route with no user, the route limit is presumably one bucket shared by
  everyone. This is read from the schema, not tested.
- So the route limit is a global ceiling, set well above expected total traffic:
  100 requests per second, replacing the 600 per minute placeholder in section
  3, which one client could use up for everyone.
- `shadowed` is a required field of `RateLimit` and is missing from the block in
  section 3. The route limit starts with `shadowed: true`.
