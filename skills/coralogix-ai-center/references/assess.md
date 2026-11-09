# Phase 1 of 4 — Assess

Analyze the repo — and the tenant, when query access is offered — to pick a maturity path and fill the one questionnaire that drives every later phase.

**Input:** repo access. Query access (a `cxup_...` key or a configured cx profile) is optional; when absent, §2's checks become questionnaire items instead.

## 1. Repo discovery

| Check | How |
|---|---|
| LLM SDKs/frameworks | Read dependency manifests (`pyproject.toml`/`requirements.txt`, `package.json`, `go.mod`, `pom.xml`/`build.gradle`, `*.csproj`) and grep the code for LLM client imports — OpenAI SDK, Anthropic SDK, LangChain, LlamaIndex, Vercel AI SDK, Bedrock, Vertex/Gemini, LiteLLM, etc. List every package that makes GenAI calls; each needs an instrumentation decision in [instrument.md](instrument.md). |
| OTel setup | Is an OTel SDK already installed and configured (tracer provider, exporter)? Does any existing instrumentation emit `gen_ai.*` attributes? |
| Collector hints | Scan for `otel-collector*.yaml`, a collector service in `docker-compose*.yml`, Helm charts/values, k8s manifests, Terraform. The repo is only a hint — collector configs commonly live in a separate infra repo, so the user is the source of truth; confirm in the questionnaire below regardless of what the scan finds. |
| Naming convention | Check mapping/config files, any collector config found above, and — with query access — `cx spans` on existing spans for `cx.application.name`/`cx.subsystem.name`. **Preserve by default.** Changing the convention is a migration that breaks existing dashboards, alerts, and saved views; it needs the user's explicit approval as a stated decision, never a side effect. |
| Gateway / proxy in the path | LiteLLM/router config, `ANTHROPIC_BASE_URL`/`OPENAI_BASE_URL` overrides, `*_BASE_URL` for sub-processes; a gateway already in the path is a rung-4 emitter candidate and the usual answer when the calling process cannot see the HTTP call (CLI/agent SDK sub-process, sandbox). |

## 2. Tenant preflight (query access only)

Run these before the questionnaire — every hit removes a question from it:

Step 0 — confirm query access exists at all:

```bash
cx profiles list
env | grep -E '^CX_(API_KEY|TOKEN|REGION)'
```

Query access exists when a profile exists or a `cxup_` key does; an env-var check alone is not the preflight — run the commands and read their errors.

```bash
cx ai-center applications list                                                                         # this app already registered?
cx spans "filter tags['gen_ai.provider.name']:string != null | limit 5" --tier archive --start now-7d  # gen_ai.* already flowing?
cx spans "filter tags['gen_ai.prompt_price']:string != null | limit 1" --tier archive --start now-7d   # any tenant span enriched → S3 archive + parsing proven
```

No query access → ask instead: is this app already registered, and is the S3 archive connected with traces routed to it?

## 3. Maturity router

| Path | Signal in repo/tenant | What [instrument.md](instrument.md) does |
|---|---|---|
| **M1** — greenfield | No OTel SDK installed, no tracer provider | Install SDK, instrumentor, and a full export path from scratch |
| **M2** — OTel present, no GenAI | Tracer provider/exporter configured, no `gen_ai.*` spans | Add the instrumentor into the existing provider; new spans join existing traces |
| **M3** — collector present | Collector config found or confirmed by the user | Add the instrumentor plus the Coralogix exporter in the collector config, and run the existing-pipeline checklist |
| **M4** — GenAI already flowing | Preflight found `gen_ai.*` spans for this app | Skip straight to [run.md](run.md) / [verify.md](verify.md); return here only for gaps the gates surface |

## 4. The one questionnaire

Batch all of this into a single structured ask (options + a recommendation per item — see the ask rules in `../SKILL.md`). Present every row. A row you judge already settled is still shown, marked *proposed* with your answer — never dropped.

| Item | Options | Recommendation |
|---|---|---|
| Who runs the app | skill-run / user-run | No default — this is the user's call |
| Environments to cover | local dev / staging / production | Cover all the user names |
| Call paths to instrument | list discovered in §1 | Recommend all of them |
| Collector reachable? | yes (where) / no | Repo hints are context only; the user's answer decides |
| Upstream-bound? | yes → vendor-neutral config / no | — |
| Tenant + region | — | — |
| S3 archive connected, traces routed to it? | yes / no / unsure → guide via docs | Required for AI Center to see anything |
| Naming convention | preserve found convention / change it | Preserve by default |
| Message-content capture allowed | per environment | ON everywhere — messages are a MUST gate; an environment excluded for legal reasons is recorded as an explicit decision |
| Emitter when several rungs are viable | gateway / in-process instrumentor / manual | gateway when one is already in the path and can be scoped to this app |

## 5. Output contract → Instrument

Hand [instrument.md](instrument.md): maturity path (M1–M4), who-runs, environments, call paths, collector answer, naming decision, tenant prerequisite status (archive connected? app registered?), upstream flag, ladder rung per call path + rungs ruled out with the lookup that ruled them out.

Zero-code eBPF capture (OBI) exists as an alternative for uninstrumented processes — see `opentelemetry/instrumentation-options/ebpf-auto-instrumentation/overview.md` — but it is out of this skill's scope.
