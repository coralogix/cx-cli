# cx CLI telemetry collector

The OTel collector that receives the CLI's usage events. Design:
[contributing/telemetry-design-draft.md](../../contributing/telemetry-design-draft.md).

| File | What it is |
|---|---|
| `config.yaml` | The collector config: rate limit, validation, sanitizing, export |
| `builder.yaml` | The `ocb` manifest for the custom collector image |
| `Dockerfile` | Builds the image from `builder.yaml` |
| `local.yaml` | Local check only: swaps the Coralogix exporter for a file and debug output |

## The deployed copy

This folder is the source of truth. The collector is deployed from
`coralogix/cx-cli-onboarding-service`, which holds copies of `config.yaml` and
`builder.yaml` under `cx-cli-telemetry-collector/`.

Nothing keeps the copies in sync. **When you change `config.yaml` or
`builder.yaml` here, copy the file to that repo in the same piece of work**,
and deploy the collector before releasing a CLI that needs the change
(design doc section 5.2).

## Checking a change

```bash
cargo build && scripts/telemetry-collector-check.sh
```

Needs Docker. It builds the image, runs it with `config.yaml` + `local.yaml`,
and checks real and hand-made events against it. CI runs it on every PR that
touches this folder or `src/`.
