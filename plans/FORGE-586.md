# FORGE-586 — CLI: print Coralogix console links after create/edit

## 0. Environment: how to run / check (read this first)

- Commands (CLAUDE.md + CI): `cargo build`, `cargo fmt --check`, `cargo clippy --locked -- -D warnings`, `cargo test --locked`, `cargo test --test e2e -- --ignored --test-threads=1` (needs `CX_API_KEY`). The `/run-tests` skill wraps these.
- **Blocker in the saga sandbox:** `cargo build` fails — `libdbus-sys` build script needs `libdbus-1-dev` + `pkg-config` (pulled in by `keyring = { features = ["sync-secret-service"] }` on non-musl Linux). No root/sudo/apt/nix here. CI installs them (`.github/workflows/{lint,test,build,e2e}.yml`), and macOS is unaffected. **Run all cargo checks somewhere those packages exist.** Full failure output: `.saga/artifacts/cli-before-state-and-blocker.txt`.
- Because of that, the before-state was read from source, and the actual unknown (console URL shape) was verified directly against the **live Coralogix web app** — evidence table in `.saga/artifacts/console-url-research.md`. Manual before/after recipe (mock API + `CX_HOME` profile) is in the first artifact.
- Before state: `cx dashboards create|replace`, `cx alerts create` and all `cx cases` lifecycle actions print only the green `print_created` / `finish_lifecycle` line on stderr; no URL anywhere in `src/`.

## 1. Key findings that shape the design (do not skip)

1. **Host shape is `https://<team>.<console_domain>/…`** — the first hostname label *is* the team (`team_url`). A team-less host (`app.eu2.coralogix.com`) is read as team "app" and lands on a "no access to selected team" screen; `dashboard.<domain>` lands on team selection. So a usable deep link needs a team subdomain.
2. **`https://<region>.app.coralogix.com/#/…`, the pattern documented today in `skills/cx-dashboards/SKILL.md` and `skills/cx-dashboards/references/deploy.md`, is wrong.** `https://eu2.app.coralogix.com/` serves the **EU1** cluster config (`cxDomainRegion: EU1`) — `eu2` is being treated as a team name. The console also no longer uses `#/` routing (`<base href="/">`). Fix those docs as part of this ticket.
3. Verified region → console domain (`__cxConfig.appUrlEnding`): us1 `app.coralogix.us`, us2 `cx498.coralogix.com`, us3 `us3.coralogix.com`, eu1 `coralogix.com`, eu2 `app.eu2.coralogix.com`, ap2 `app.coralogixsg.com`, ap3 `ap3.coralogix.com`. **ap1 and stg1 unverified** (DNS unreachable from the sandbox).
4. Verified entity paths: dashboard `/dashboards/<id>`, alert `/alerts/<id>`, case `/cases?id=<case-id>` (query param, not a path segment; the `/cases` route is feature-flagged `apollo-cases`). Which case id the param expects (`id` vs `readableId`) is **unverified**.
5. `ResolvedConfig` carries only `endpoint: String`, **not** the `Region`, and `Profile` has **no** endpoint-override field (a custom endpoint is `Region::Custom(url)`). So the console base has to be resolved during config resolution and plumbed onto `ResolvedConfig`.
6. `src/identity.rs::resolve_team_id` already calls `GET /identity/whoami` (returns `team_id`, `team_name`), and `cx cases assign` already makes such a call — precedent for a best-effort identity lookup exists.

## 2. Changes, in dependency order

### Step 1 — `src/config.rs`: region → console domain

```rust
impl Region {
    /// Base domain of the Coralogix web console for this region, WITHOUT the
    /// team subdomain. Console URLs are `https://<team>.<console_domain>/…`.
    /// `None` when no console host can be derived (custom / self-hosted).
    pub fn console_domain(&self) -> Option<&'static str> { … }
}
```
Use the verified table from §1.3. `Region::Custom(_) => None`. For `Ap1`/`Stg1`: try to verify with `curl -s https://app.coralogix.in/ | grep -o '"appUrlEnding":"[^"]*"'` (and a stg1 host); if still unverifiable return `None` and list the gap in the PR description per the parent ticket's "skip and report" instruction. Add a doc comment pointing at how the values were obtained (`__cxConfig.appUrlEnding`).

Unit tests: one per region + `Custom` → `None`.

### Step 2 — profile override + `ResolvedConfig` plumbing

- `Profile`: add `#[serde(default, skip_serializing_if = "Option::is_none")] pub console_url: Option<String>` — a full console base **including** the team subdomain, e.g. `https://c4c.app.eu2.coralogix.com`. This is the escape hatch for `Region::Custom`, self-hosted/IBM installs, and any team whose subdomain differs from its name. No change to the `cx profiles add` wizard (out of scope); document manual editing instead.
- `ResolvedConfig`: add
  - `pub console_url: Option<String>` (explicit override, trailing `/` trimmed),
  - `pub console_domain: Option<String>` (from `region.console_domain()`).
  Populate in **both** constructions in `resolve_single` (`src/config.rs:446` env-only path and `:512` profile path).
- Update the two literal constructions that will break: `src/config.rs:704` (unit test) and `tests/common/mod.rs:26` (`test_target*` helper) — default both new fields to `None` so every existing test keeps today's behaviour. Consider adding `test_target_with_console(profile, base_url, console_url)` for the new integration tests.

### Step 3 — `src/identity.rs`: hedge on the team subdomain field

Add `#[serde(default)] pub team_url: Option<String>` to `Whoami` (harmless if the API doesn't return it) and a helper:

```rust
/// Team subdomain label for console URLs. Prefers `team_url`, falls back to
/// `team_name`. Returns None when the value is not a valid hostname label.
pub async fn resolve_team_subdomain(client: &CxClient) -> Option<String>
```
Lowercase, and reject anything that isn't `[a-z0-9-]+` (never emit a broken/ambiguous host). Never propagate an error — a missing/forbidden whoami must not fail the mutation.

### Step 4 — new `src/console_url.rs` + a render helper

Pure builders (unit-testable, no I/O):
```rust
pub fn dashboard_url(base: &str, id: &str) -> String;  // {base}/dashboards/{id}
pub fn alert_url(base: &str, id: &str) -> String;      // {base}/alerts/{id}
pub fn case_url(base: &str, id: &str) -> String;       // {base}/cases?id={urlencoded id}
```
Trim a trailing `/` from `base`; percent-encode ids (the `url` crate is already a dependency).

Resolution of the base, cached per target:
- `ExecutionTarget` (`src/execution.rs:24`) gains `console_base: tokio::sync::OnceCell<Option<String>>` and `pub async fn console_base(&self) -> Option<String>`:
  1. `cfg.console_url` → `Some(...)` (no HTTP call),
  2. else `cfg.console_domain` → `resolve_team_subdomain(&self.client).await` → `Some(format!("https://{team}.{domain}"))`,
  3. else / on any failure → `None`.
  `OnceCell` guarantees at most one `/identity/whoami` per profile per command run.
- `src/render.rs`, next to `print_created`: `pub fn print_console_link(url: &str)` → `eprintln!("{}", format!("View in Coralogix: {url}").cyan())`. **stderr only** — never added to `render_json*` / `render_agents` payloads, so `--output json|agents` stdout is byte-identical to today.

### Step 5 — wire into the handlers

- `src/commands/dashboards/mod.rs::run_create` (after `print_created`, only when `created_id.is_some()`) and `run_replace` (same, `replaced_id`).
- `src/commands/alerts/mod.rs::run_create` — `alert.id`.
- `src/commands/cases/mod.rs::finish_lifecycle` — add two params (`targets: &[Arc<ExecutionTarget>]`, `case_id: &str`), look the target up by profile name, print the link after the green success line. Update all 9 call sites (`run_update`, `run_comment`, `run_assign`, `run_unassign`, `run_acknowledge`, `run_unacknowledge`, `run_resolve`, `run_close`, `run_set_priority`). This covers the five actions the ticket lists plus the other lifecycle verbs that share the helper — intended, mention it in the PR. `finish_lifecycle` becomes `async`.
- One link per profile in multi-profile fan-out, printed right after that profile's success line.

### Step 6 — docs & skills

- `docs/configuration.md`: document the new `console_url` profile field and add the region → console-domain table next to the existing region → API endpoint table (§ around line 188).
- `example_config.toml`: mention `console_url` in the profile example comment block.
- `skills/cx-dashboards/SKILL.md` (§ "Output format for the user", ~line 290-320) and `skills/cx-dashboards/references/deploy.md` (~line 60-85): replace the incorrect `https://<region>.app.coralogix.com/#/dashboards/<id>` pattern and its region table with "use the `View in Coralogix:` link the CLI prints on stderr after `cx dashboards create|replace`; if the CLI prints no link, omit the link — do not invent a URL." Keep the "no link ⇒ drop the link line" template.

## 3. Tests

- **Unit** (`src/config.rs`): `console_domain()` per region + `Custom` → `None`.
- **Unit** (`src/console_url.rs`): builders — no double slash, trailing-slash trim, id percent-encoding, `/cases?id=` shape.
- **Unit** (`src/identity.rs`): `Whoami` deserializes with and without `team_url`; subdomain helper prefers `team_url`, falls back to `team_name`, rejects invalid labels.
- **Integration, binary-level** — new `tests/console_urls/main.rs` (auto-discovered; follow the `assert_cmd` + `wiremock` + `CX_HOME` pattern in `tests/profile_override/main.rs`). Profile: `region = "<mock uri>"` (⇒ `Custom`) **plus** `console_url = "https://acme.app.eu2.coralogix.com"` — this is exactly why the override exists and is the only way to exercise link printing against a mock, since `Profile` has no endpoint override. Cases:
  1. `dashboards create` → stderr contains `https://acme.app.eu2.coralogix.com/dashboards/dash-abc123`; with `-o json`, stdout contains no `url`/link key.
  2. `dashboards replace`, `alerts create`, one `cases` lifecycle action (e.g. `resolve`) → correct link each.
  3. `console_url` set ⇒ **no** `GET /identity/whoami` request (`Mock…expect(0)`).
  4. No `console_url` and a `Custom` region ⇒ command succeeds, stderr has no `View in Coralogix` line.
  5. whoami mocked as 403 with a region-derived domain ⇒ command still succeeds and prints no link (verify via a profile whose console domain is derivable — if that's not expressible with a mock endpoint, cover it with a unit test on `console_base` instead).
- **E2E** (`tests/e2e/…`, `#[ignore]`d): only if the stg1 console domain gets verified; otherwise leave e2e untouched.

## 4. Edge cases / risks

- `Region::Custom(_)`: no derivable console host → no link (explicit ticket requirement); `console_url` is the opt-in.
- **Team subdomain ≠ team name** is the main correctness risk. Mitigations: prefer whoami's `team_url` if present, `console_url` override, and skip rather than guess. Implementer should run `cx` against the E2E test team (or dump `GET /identity/whoami`) and confirm the built host actually loads the entity; if `team_name` proves unreliable, state that in the PR and lean on `console_url`.
- **`/cases?id=` id semantics unverified** — confirm with a real case that `id` takes the case UUID the CLI accepts. If it needs `readableId`, use `case.readableId` from the response; if neither resolves, drop the cases link and report it in the PR (parent-ticket escape hatch) rather than shipping a dead link.
- ap1/stg1 console domains unverified → `None` + PR note unless verified.
- Missing entity id in the API response → no link (`print_created` already warns about that case).
- Extra HTTP call: at most one `/identity/whoami` per profile per run, only on create/edit commands, only when no `console_url` override; failures are swallowed and must never change the command's exit code.
- Do not touch `render_json`/`render_agents` payloads; verify stdout is unchanged.
- Keep `cargo fmt` clean and `cargo clippy -- -D warnings` warning-free (CI gate).

## 5. Verification

1. `cargo fmt --check && cargo clippy --locked -- -D warnings && cargo test --locked` (in an env with `libdbus-1-dev` + `pkg-config`).
2. Manual before/after with the mock recipe in `.saga/artifacts/cli-before-state-and-blocker.txt`:
   - before: `Created dashboard 'Demo Dashboard' (ID: dash-abc123) in profile 'mock'.`
   - after: same line **plus** `View in Coralogix: https://acme.app.eu2.coralogix.com/dashboards/dash-abc123`
   - `-o json` stdout diff (stdout only, stderr discarded) must be empty before vs after.
3. Against a real team (region `eu2`, no `console_url`): create a dashboard, an alert, and mutate a case; open each printed link and confirm it lands on the entity. Capture the terminal output as the after-state artifact.
4. PR description must list what was skipped and why: `Region::Custom` (no derivable host), any unverified region domain (ap1/stg1), and cases if the `/cases?id=` link can't be confirmed.
