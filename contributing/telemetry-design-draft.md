# cx CLI telemetry — draft

Status: draft. The CLI side (section 1) is implemented in `src/telemetry/`. The
collector config (section 2) passes `otelcol validate` and a local end-to-end run
against `otel/opentelemetry-collector-contrib:0.165.0`: see
`scripts/telemetry-collector/config.yaml` and
`scripts/telemetry-collector-check.sh`. Sections 3 and 4 are not done.

```
cx command → local spool → HTTPS POST (OTLP/HTTP JSON) → API gateway (open path)
           → dedicated OTel collector (validate, relabel) → c4c
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
- Any HTTP response means the batch is finished and removed from the spool.
- A network failure keeps the batch until it is older than 7 days.
- At most 50 events per request.

## 2. Collector config

A separate collector deployment, used only for CLI events.

The config is `scripts/telemetry-collector/config.yaml`.

What the collector does not do, and where it is handled:

| Gap | Handled by |
|---|---|
| Per-IP rate limit | API gateway rate limit on the route (to confirm) |
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
      unit: minute
      requestsPerUnit: 600            # placeholder
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

- Does the gateway rate limit work per source IP on an unauthenticated endpoint? The
  schema only shows `userLevelScope`, which suggests per-user limits.
- Which cluster hosts the collector, and what fixed hostname does the CLI use before
  the user has a region?
- Who owns the collector deployment?
- Is a client-claimed `team_id` acceptable, or is a second authenticated endpoint
  needed?
- Can an `application` deployable run with only `container.image` and no build? The
  only repo that ships a collector as its own deployable
  (`eng-onlineboutique-demo/otelcollector`) builds an image with the config copied in.
- Which `otelcontribcol` tag to pin? Ask the Observability team, who own the
  existing collectors.
- `error_mode: propagate` on the validation processors is deliberate. The internal
  OTTL skill recommends `ignore` for production, but with `ignore` a check that
  errors keeps the record, which lets bad data through. Confirm with Observability.
