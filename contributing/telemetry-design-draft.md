# cx CLI telemetry — draft

Status: draft. The CLI side is implemented in `src/telemetry/`. Section 5 holds
the decisions from the 2026-11-16 design review; where it differs from sections
1 to 4, section 5 wins.

The dedicated OTel collector of the first draft was dropped on 2026-11-19. The
CLI now gets a send-your-data key from the onboarding service and sends its
events straight to Coralogix (section 2).

```
cx command → local spool → detached sender
           → GET the key (onboarding service, open route, cached 24 h)
           → HTTPS POST (OTLP/HTTP JSON, Bearer key) → Coralogix ingress → c4c
```

## 1. Event schema

Each event is one OTLP log record. Every value is an enum, a UUID, a number, a
boolean, or a string matched by a strict pattern. There is no free text, and the log
body is empty.

### Resource attributes (same for every event in a request)

| Attribute | Type | Allowed values | Source in the CLI |
|---|---|---|---|
| `cx.application.name` | string | `cx-cli` | constant. The application the logs land under |
| `cx.subsystem.name` | string | `usage` | constant. The subsystem the logs land under |
| `service.name` | string | `cx-cli` | constant |
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
Nothing corrects a wrong client clock any more (the collector did): an event
from a machine with a wrong clock is stored with that wrong time.

### Events

| `event.name` | When | Extra attributes |
|---|---|---|
| `cli.first_run` | `~/.cx/state.json` is created | none |
| `cli.command` | a command the user ran finishes (not the background `cx skills auto-refresh`, which records nothing) | command set below |
| `cli.parse_error` | clap rejects the arguments | `cx.parse.kind`: `unknown_command`, `unknown_flag`, `missing_arg`, `other` |
| `cli.onboarding` | an onboarding step happens | `cx.onboarding.step`: `init_started`, `init_completed`, `profile_added`, `oauth_started`, `oauth_succeeded`, `oauth_failed`, `first_query_succeeded`; `cx.auth.method`: `oauth`, `api_key`; `cx.credential.store`: `keychain`, `file`. `profile_added` is for a new profile only, not for one that is overwritten |
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
| `cx.http.status_class` | string | `none`, `2xx`, `4xx`, `5xx`: the class of the highest HTTP status the Coralogix API answered with in this run. `none` when it answered nothing |
| `cx.duration_ms` | int | 0 to 3,600,000 |
| `cx.output.format` | string | `text`, `json`, `toon` |
| `cx.profile.count` | int | 0 to 50 |
| `cx.spilled` | bool | |

`cx.command.flags` is a joined string, not an array, so one pattern checks
it.

`cx.http.status_class` is observed, not inferred from the outcome. A fan-out
that succeeds overall although one profile got a 500 reports `success` with
`5xx`. An error raised before any request (a malformed API key) reports `none`.

**Known limit: interrupted runs.** `cli.command` is recorded when the command
returns. A run stopped with Ctrl-C, or killed, records nothing. Users interrupt
slow and hung commands, so those are under-counted: durations and error rates
look better than they are. `cx` installs no signal handler to change this.

### Example request

`POST <host_url>/v1/logs`, `Content-Type: application/json`,
`Authorization: Bearer <key>` (section 2)

```json
{
  "resourceLogs": [{
    "resource": { "attributes": [
      { "key": "cx.application.name", "value": { "stringValue": "cx-cli" } },
      { "key": "cx.subsystem.name", "value": { "stringValue": "usage" } },
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
- A 2xx, or a 4xx other than 401, 403, 408 and 429, means the batch is
  finished and removed from the spool. Any other HTTP response keeps it.
  Redirects are not followed. A 401 or 403 means the key was refused
  (section 2).
- A network failure keeps the batch until it is older than 7 days.
- At most 50 events and 60 KiB of body per request, whichever is reached
  first. The ingress takes far larger bodies (10 MB); the cap only keeps
  requests small. 50 typical events are about 47 KB;
  50 events with 30 flags of 40 characters are about 110 KB and go out as two
  requests.
- An event too large to fit a request on its own is dropped, not sent, so it
  cannot hold back the events behind it. No event the CLI builds is that large.
- A spool line is checked again when the sender reads it: every enum and every
  constrained string (command path, flags, agent, team id, versions) must pass
  the check it was built with. A line that fails is dropped, so an edited spool
  cannot put free text in a request.
- No command sends. A detached sender sub-process does, after the command has
  exited (section 5.1).

## 2. Transport: the key route and the Coralogix ingress

There is no service of our own between the CLI and Coralogix. The sender
sub-process (section 5.1) does two things: it gets a key, and it posts the
events with it. No command does either.

### The key route

`GET https://api.eu2.coralogix.com/api/v2/cli-telemetry-key`, served by
`cx-cli-onboarding-service` (coralogix/cx-cli-onboarding-service#12). No auth:
the CLI calls it before the user has signed in. The CLI always calls the eu2
host, whatever region the user's profile points at.

| Answer | Meaning |
|---|---|
| `200 {"key": "...", "host_url": "https://..."}` | a send-your-data key of a team used only for CLI telemetry, and the ingress host to send to |
| `503` | no key is stored yet |
| `404` | the route is not deployed yet |
| `429` | the gateway limit: one bucket of 100 requests per second for all callers together |

The key is public. Anyone can fetch it, so it is never treated as a credential
of the user. It is still never printed or logged: the type that holds it has no
`Debug`, and the sender has no stdout or stderr.

### The key cache

The answer is cached in `~/.cx/telemetry-key.json` with its fetch time and used
for 24 hours. Without a cache every command would call a route whose limit is
shared by all users. 24 hours was chosen because a rotation does not wait for
the cache (see "key refused" below), so a long cache costs nothing.

A cache file that does not parse, is older than 24 hours, has a fetch time in
the future, or holds a host that fails the check below counts as no key.

### The host check

The service stores `host_url` as typed in by hand and does not check it. The
CLI sends only to a `host_url` that is `https://`, has no path, query or
credentials, and whose host is, or ends with a dot followed by, one of:
`coralogix.com`, `coralogix.us`, `coralogix.in`, `coralogix.net`,
`coralogixsg.com`. Anything else counts as a failed fetch. Debug builds also
accept `http://` on a loopback address, so the tests can use a mock server.

### Sending

`POST <host_url>/v1/logs` with `Authorization: Bearer <key>` and the OTLP/HTTP
JSON body of section 1. This is the documented Coralogix OpenTelemetry endpoint
(`cx docs fetch developer-portal/apis/data-ingestion/opentelemetry-custom-logs.md`).
The logs land under application `cx-cli`, subsystem `usage`.

### What the sender does with each result

| Result | Sender does |
|---|---|
| Ingress 2xx | batch removed, backoff file removed |
| Ingress 401 or 403 (key refused) | delete the cache, fetch the key once, post the batch once more. If that fails too, keep the batch and back off. Once per sender |
| Ingress other 4xx (not 408, 429) | batch removed: it would be rejected again |
| Ingress 3xx, 408, 429, 5xx, network error, timeout | batch kept, back off |
| Key route 404, 429, 503, any non-200, network error, a body that is not the expected JSON, an empty key, a host that fails the check | nothing is taken from the spool, nothing is cached, back off |

"Back off" is the 15-minute backoff of section 5.1. Nothing is ever printed.

No jitter is added to the backoff. A retry is not on a timer: it happens on
the user's next command after the 15 minutes, and those are already spread
out. Add jitter if the route's limit is ever reached in practice.

### What is gone with the collector

| Was done by the collector | Now |
|---|---|
| Allow-list of every event name, attribute and value | **nothing on the server**. The types in `event.rs` are the only limit on what the CLI sends. Anyone who holds the key can write any log into the telemetry team |
| Per-IP rate limit | none. The control is the daily quota of the telemetry team |
| Correcting a wrong client clock | none |
| Setting application and subsystem | the CLI sends `cx.application.name` and `cx.subsystem.name` |
| Duplicate events | as before: deduplicate on `cx.invocation.id` + `event.name` + the record timestamp in c4c queries |
| Verified `team_id` | as before: not available. `cx.team.id` is client-claimed |

Queries and dashboards on the telemetry team must not trust a field's value
without checking its shape, because the key is public.

## 3. Rollout

- The server side is in coralogix/cx-cli-onboarding-service#12. Until it is
  deployed the route answers 404, and until the owner stores a key it answers
  503. The CLI is silent and harmless in both cases: events wait in the spool
  for up to 7 days and are then dropped.
- No build sends until `BUILD_KEY_URL` in `src/telemetry/consent.rs` is set
  (section 5.2).
- The key must belong to a team used only for CLI telemetry, with a daily
  quota.

## 4. Open questions

- Is a client-claimed `team_id` acceptable?
- The one-time notice is printed to stderr once, wherever `cx` runs, and then
  marked as shown. Under an agent or in CI no person reads it, so that user is
  never told. Is that acceptable, or must recording wait until the notice has
  been printed to a terminal? Product/Legal to decide before any build gets a
  key route URL.
- With no server-side validation, is the quota of the telemetry team enough
  protection against someone writing junk with the public key?

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
- The sender reads the consent mode and the key route URL itself, the same way `cx`
  does. An opt-out set after the spawn still stops it.

**When it starts**

- At the end of the run, right after `cli.command` is appended, so this run's
  event goes out with this sender.
- Also before clap exits on a parse error, which never reaches the end of `main`.
- Only in `On` mode. With any opt-out there is no spool and no sender.
- Starting it at the beginning of the run was rejected: the spawn costs the same
  either way, and this run's event would wait for the next run.
- A minimum interval between sends was rejected. The cost of sending after
  every command is one spawn and one request per command, so an agent loop
  starts twice as many processes and batches rarely fill. With an interval, a
  CI job or container that runs several commands and is then destroyed would
  deliver only the first command's events.

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

- The sender touches it when the batch is kept (see below) or when it could
  not get a key (section 2), and deletes it after a batch is accepted.
- The parent does not spawn while the file is younger than 15 minutes.
- Result: with Coralogix or the key route unreachable, one sender per 15
  minutes per machine.

**What ends a batch**

| Result | Batch | Backoff |
|---|---|---|
| HTTP 2xx, or 4xx other than 401, 403, 408 and 429 | removed | no |
| HTTP 401 or 403, after one fetch of a new key and one more try | kept | yes |
| Any other HTTP status (3xx, 408, 429, 5xx) | kept | yes |
| Network error or timeout | kept | yes |
| No key (section 2) | not taken from the spool | yes |

A kept status is one that does not show the ingress looked at the batch.
Redirects are not followed: following one re-sends the POST as a GET with no
body, possibly to another host.

**Timeouts**

- Connect 5 s, whole request 10 s, for the key fetch and for each post.
- The sender sends batches of up to 50 events and 60 KiB (section 1, client
  rules) until the spool is empty. The spool holds at most 500 events: 10
  requests when they are typical, about 20 when every event is the largest
  the CLI can build.
- Lifetime cap 30 s, enforced by a watchdog thread that exits the process. A DNS
  lookup runs on a blocking thread that a request timeout cannot stop. The cap
  stays below the 60 s after which a `.sending` file counts as abandoned.

**The runtime shutdown in `main`**

The bounded 100 ms `shutdown_timeout` was added only for the in-process flush.
With no flush it is removed, and `main` goes back to `#[tokio::main]` as on
master, with the wrapper logic (`begin`, `run`, `finish`, spawn) inside it.

**Removed from the command path**

`telemetry::flush`, the `network` gate on flushing, `send::BUDGET`,
`Invocation.network` and `mark_network`. `cx.http.status_class` comes from the
statuses `CxClient` sees (`telemetry::observe_status`), not from whether the
command was expected to use the network.

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
| Server refuses the connection | 10.8 ms | 11.1 ms |
| Server accepts and never answers | 11.0 ms | 11.5 ms |
| DNS stalls (a `.local` name, 5 s mDNS timeout) | 10.9 ms | 11.7 ms |

Telemetry adds 0.4 to 0.6 ms at the median and under 1 ms at p95, whatever the
server does (measured with the collector transport; the command path has not
changed since). That includes building the event and appending it to the
spool. With the in-process flush, the DNS case took about 700 ms.

One outlier: the first spawn of the freshly downloaded `cx.exe` on Windows took
82 ms, once in 600 spawns. That is most likely the antivirus scan of a new
binary, so it can happen on the first run after an install or upgrade.

### 5.2 The key route URL and release order

This section used to tie CLI releases to the collector's config. With no
collector there is no config to be behind, so the `telemetry-contract` CI jobs,
the release gate script and the local collector were removed.

**The key route URL is a constant in the source**, `BUILD_KEY_URL` in
`src/telemetry/consent.rs`, not a build-time variable. Homebrew compiles the
tagged source on its own machines (`cargo install`, no network), and Cargo and
Nix builds do the same, so a repository variable or secret never reaches them.
The URL is not a secret: it ships in a public binary. Debug builds ignore the
constant, so tests and `cargo run` never call the real route. `.cargo/config.toml`
also sets `CX_TELEMETRY=0` for everything cargo runs, so a
`CX_TELEMETRY_KEY_URL` exported in a developer's shell is not used by the
test suites either; `tests/telemetry` sets its own values.

While the constant is `None`, no release sends. Setting it is the switch, and
it does not depend on the server: a release that calls a route that is not
deployed (404) or has no key (503) stays silent.

**Schema changes** still only add: a new event, a new attribute, a new enum
value. Nothing checks this on the server any more, but old CLI versions keep
sending the old shape for as long as they are installed, and the queries on
the telemetry team must keep working for them.

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
| Spec, section 1: "string, optional" | no |

Gaps (1 and 2 are fixed):

1. The collector check script never sent an event that has a team id. The
   script is gone with the collector; the unit test below covers the encoding.
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

**On a release build, `CX_TELEMETRY=log` means off.** Any value other than
`1`, `true`, `on`, `yes` or empty opts out, so someone who set `log` to make
sure nothing is sent does not start sending because the mode was removed.

**Tests**

- `tests/telemetry/main.rs` keeps using `CX_TELEMETRY=log`. The log-mode tests
  get `#[cfg(debug_assertions)]`, so `cargo test --release` skips them and does
  not fail.
- One new unit test on `consent::decide` covers both builds: `log` gives `Log`
  with debug assertions and `Off` without.

**For users and reviewers on a release build**

1. `docs/telemetry.md` (M5) lists every field and every value.
2. A release build has no way to show the real bytes locally. The old way,
   pointing the endpoint at a local listener, no longer works: the CLI now
   needs a key route to answer first, and a release build sends only to an
   `https://` Coralogix host (section 2). To see the bytes, use a debug build
   with `CX_TELEMETRY=log`.

No new command is added for this.

### 5.5 Rate limits

The per-IP rate limit lived in the collector and is gone with it. What is left:

- The key route has one gateway limit for all callers together (100 requests
  per second). The platform has no per-IP scope for an unauthenticated route.
  The 24-hour key cache and the 15-minute backoff keep each machine far below
  it: at most one call per day when all is well, and at most one per 15
  minutes while the route has no key.
- Ingestion is limited only by the daily quota of the telemetry team.
