# FORGE-186 — Add Olly Mini to AI Units data usage page

## Summary of the change

The AI Units data-usage page fetches Olly usage via
`UnifiedDataUsageService.getOllyUsageByDateRange` → `#streamOllyTeamUsage` (gRPC
server-stream `OllyUsageServiceDefinition.getOllyDailyTeamUsage`) →
`#buildOllyUsageData`. Today the aggregator reads the **deprecated**
`response.dailyUsage` wrapper (`OllyUserDailyUsage`, `deprecated=true`), which
back-end-side is Olly-only. The non-deprecated equivalents on the same response
message — `response.date` + `response.userUsages: OllyUserUsage[]` — carry both
`OllyType.OLLY_TYPE_OLLY` **and** `OllyType.OLLY_TYPE_OLLY_MINI` dimensions
(both enum values already exist in `libs/protos/.../v3/olly_usage.ts`).

Because the inner loop `for (const dim of userUsage.usage)` in
`#buildOllyUsageData` already sums every `OllyUsageByDimension` without
inspecting `dim.ollyType`, migrating the outer field read from
`response.dailyUsage.{date,usage}` to `response.{date,userUsages}` is sufficient
to naturally union both `OllyType` values into daily totals, per-user rows,
per-user-per-dimension rows, and per-day dimension breakdowns — with zero
UI/visual change and no proto/wire-contract change. `buildAiUnitsTotalUsageData`
in `ai-units-aggregation.utils.ts` sums `ollyUsage.dailyTotals`, so the composite
"Total" chart picks the new numbers up automatically.

## Files to change

- **`frontend/libs/settings/core/src/lib/shared/services/unified-data-usage.service.ts`**
  Modify `#buildOllyUsageData` (lines ~500–691): swap the outer read from
  `response.dailyUsage` → `response.date` + `response.userUsages`. Everything
  else in that method stays untouched.

- **`frontend/libs/settings/core/src/lib/shared/services/unified-data-usage.service.spec.ts`** *(new)*
  Add a targeted spec (see "Testing" below) exercising `getOllyUsageByDateRange`
  end-to-end with a fake `#ollyUsageClient` streaming a response that contains
  both `OLLY_TYPE_OLLY` and `OLLY_TYPE_OLLY_MINI` dimensions, asserting they
  land in the unified totals/rows/dimensions.

No changes to:
- `data-usage/utils/olly-usage-transformer.ts` — operates on the already-unified
  `OllyUsageData` shape.
- `data-usage/utils/ai-units-aggregation.utils.ts` — sums whatever
  `ollyUsage.dailyTotals` contains.
- `data-usage/data-usage.component.ts` — just consumes `OllyUsageData`.
- `data-usage/data-usage.types.ts` — the `OllyUsageData` shape is unchanged.
- Proto files under `libs/protos/` — `olly_type` is already defined; the
  non-deprecated response fields are already generated.

## Exact edit inside `#buildOllyUsageData`

Current (unified-data-usage.service.ts:532–543):

```ts
for (const response of responses) {
  const dailyUsage = response.dailyUsage;
  if (!dailyUsage?.date) continue;

  const date = calendarDateToUtc(dailyUsage.date);
  const dateKey = toIsoDateKey(date);

  if (!dailyDimensions[dateKey]) {
    dailyDimensions[dateKey] = [];
  }

  for (const userUsage of dailyUsage.usage) {
    // ... unchanged
  }
}
```

Change to:

```ts
for (const response of responses) {
  if (!response.date) continue;

  const date = calendarDateToUtc(response.date);
  const dateKey = toIsoDateKey(date);

  if (!dailyDimensions[dateKey]) {
    dailyDimensions[dateKey] = [];
  }

  for (const userUsage of response.userUsages) {
    // ... unchanged
  }
}
```

Everything from `for (const dim of userUsage.usage)` inward is untouched — the
existing per-dimension loop already blindly sums each `OllyUsageByDimension`
regardless of `ollyType`, so `OLLY` and `OLLY_MINI` dimensions naturally union
into `dailyTotals`, `dailyTokenTotals`, `perUserDailyUnits`,
`perUserDailyTokens`, `perUserDimDailyUnits`, and `dailyDimensions`.

## Order of changes

1. Update `#buildOllyUsageData` as shown above.
2. Add the new `unified-data-usage.service.spec.ts` — see below.
3. Run local checks (see "Verification").

There are no cross-file dependencies to sequence — the aggregation surface
(`OllyUsageData`) is unchanged, so downstream consumers keep working.

## Edge cases and risks

1. **Backward compatibility with a stale backend.** The proto docstring on
   `GetOllyDailyTeamUsageResponse.dailyUsage` says it is "Legacy deprecated
   daily wrapper kept for backwards-compatible clients." The ticket description
   states the wire schema already carries `ollyType` on the non-deprecated
   fields, and the sibling `cx-llm-gateway` PR #103 / #110 (FORGE-62 / FORGE-189
   context) confirms the backend side has been aggregating both OllyTypes.
   Implementation should nevertheless **verify** on staging that
   `response.userUsages` is populated (see Step 2 of Verification) before
   merging — if a stale backend region only fills `dailyUsage`, we would
   silently render empty per-user rows there.

2. **`quota-rules.service.ts` shares this fetch.**
   `#fetchOllyUsageData` in
   `libs/settings/core/src/lib/quota-rules/services/quota-rules.service.ts:414`
   calls `getOllyUsageByDateRange` and maps `ollyData.dailyTotals` into
   `EntityTypeUsageData` under `EntityType.ENTITY_TYPE_OLLY`. After this change,
   that "OLLY" entity-type row on the Quota Allocation Rules page will also
   include Olly Mini usage. This is consistent with the org-wide "Unify all AI
   usage (non-olly) to consume AI units" project and the ticket's stated
   framing that Olly Mini is Olly-family usage, but is worth calling out in the
   PR description so reviewers can confirm the intent. **Do not** split the
   fetch — the ticket explicitly limits the change to the fetch/aggregation
   layer, and having two paths (unified vs. Olly-only) would fragment the
   source of truth. If reviewers push back, the fallback is to add an
   `includeOllyMini: boolean` parameter to `getOllyUsageByDateRange` (default
   true) and have quota-rules pass `false`; do this only if requested.

3. **`keyUsages` still ignored.** `GetOllyDailyTeamUsageResponse` also carries a
   `keyUsages: OllyTeamKeyUsage[]` field for API-key-attributed usage. The
   current code doesn't handle keys and the ticket's success criteria are
   phrased around **per-user** rows / dimension breakdowns / daily totals
   ("daily/per-user Olly usage numbers … per-user rows, and per-dimension
   breakdowns"). Continue ignoring `keyUsages` in this ticket; adding key-scoped
   totals is a separate scope decision and would change the UI (new row type)
   which is explicitly out of scope.

4. **Deprecated `dim.units`/`dim.tokens` fallback.** The per-tier logic
   (`freeUnits`/`paidUnits`/`freeTokens`/`paidTokens` + deprecated fallback to
   `units`/`tokens`) is unchanged and still correct — both OllyType dimensions
   carry the same tier fields.

5. **Empty response defensiveness.** If both `response.date` and
   `response.userUsages` are empty for a given streamed message, the `continue`
   on `!response.date` already short-circuits — no NPE risk.

## Testing

### Existing tests to re-run (should stay green)

- `olly-usage-transformer.spec.ts` — operates on the `OllyUsageData` shape,
  which is unchanged.
- `ai-units-aggregation.utils.spec.ts` — operates on `OllyUsageData`.
- Any other `settings-core` spec that references Olly usage.

### New spec — `unified-data-usage.service.spec.ts`

Because `#buildOllyUsageData` is private, drive it through the public
`getOllyUsageByDateRange` observable. The gRPC client is created via
`GrpcClientService.clientFactory`, so stub `GrpcClientService` in the TestBed
providers to return a fake `#ollyUsageClient` whose `getOllyDailyTeamUsage`
returns an async-iterable emitting one or more
`GetOllyDailyTeamUsageResponse` objects.

Follow the repo's Single-Action-Test pattern (`.claude/rules/single-action-test.md`)
and Vitest Angular service conventions
(`.claude/rules/vitest-angular-services-unit-tests.md` — no `import
{describe,it,expect}` from vitest, `vi.fn()` for mocks, use `createVitestConfig`
which is already set up for `settings-core`).

Minimum test coverage:

```
describe('UnifiedDataUsageService', () => {
  describe('getOllyUsageByDateRange', () => {
    it(`GIVEN a stream response containing one OLLY dimension and one
        OLLY_MINI dimension for the same user and day
        WHEN aggregating
        THEN dailyTotals sums both dimensions`, ...);

    it(`GIVEN a stream response containing only OLLY_MINI dimensions
        THEN per-user rows include the Olly Mini usage`, ...);

    it(`GIVEN a stream response containing only OLLY dimensions
        THEN totals match the OLLY-only sum (no regression)`, ...);

    it(`GIVEN a stream response reading response.date and response.userUsages
        (non-deprecated fields)
        THEN the aggregator produces a day entry for that date
             (proves we switched off the deprecated wrapper)`, ...);
  });
});
```

The fourth test is the key regression guard for the wire-field switch: feed a
response where `response.dailyUsage` is `undefined` but `response.date` +
`response.userUsages` are populated, and assert the aggregator still produces
data. This will fail against the current (pre-change) code path.

Stubbing tip: `#ollyUsageClient` is obtained from
`grpcClientService.clientFactory(OllyUsageServiceDefinition, ...)`, so provide
`{ provide: GrpcClientService, useValue: { clientFactory: () => fakeClient } }`
and let `fakeClient.getOllyDailyTeamUsage` return an `async function*` yielding
your fixtures. Also stub `AuthService`, `HttpClient`, and `Store` with minimal
values (Store's `getTeammates` selector should return `[]`).

## How to run / check (from `frontend/` root)

Established from `frontend/CLAUDE.md` and rules:

- **Lint the touched project:** `pnpm nx lint settings-core`
- **Unit-test the touched project:** `pnpm nx test settings-core`
  - Focused: `pnpm nx test settings-core -- --testNamePattern="getOllyUsageByDateRange"`
- **Broader safety net (optional but cheap):** `pnpm nx affected -t test`
- **Type-check (via build):** `pnpm nx build web-app` — only if a broader
  compile check is desired; unit tests already type-check the touched files
  via the settings-core `tsconfig.spec.json`.

## Observing the "before" state

**This is a pure data-fetch layer change with no UI/visual change** (per the
ticket's explicit success criteria: "the exact same layout as today, but daily
totals, per-user rows, and per-dimension breakdowns include usage currently
attributed only to Olly (now unioned with Olly Mini)"). "Before" and "after"
therefore look identical at the DOM level — the diff is quantitative in the
numbers themselves.

Running the app locally (`pnpm run dstaging`) requires staging credentials that
aren't available in this worktree, and even with a working staging login, the
delta between "before" and "after" is only visible if the tenant has non-zero
Olly Mini usage in the selected date range. Absent that, the numbers would
appear identical.

Given the above, verification relies on:

1. **Unit-test coverage** — the new spec asserts the switch from
   `dailyUsage` to `userUsages`/`date` and that OLLY_MINI dimensions flow
   through into `dailyTotals`/`rows`/`userDimensionRows`/`dailyDimensions`.
2. **Post-deploy staging smoke** — after merging, load
   Settings → AI Units → Data Usage on a tenant known to have Olly Mini
   usage (e.g. Cases / RUM / AI Center coding-agent activity), and confirm
   the Olly section daily totals and per-user rows increased vs. the same
   range pre-merge. Also confirm the composite "Total" chart in the AI Units
   tab increased by the same delta (proves `buildAiUnitsTotalUsageData`
   picked up the correction with no code change there).
3. **No-regression check** — on a tenant without any Olly Mini usage, numbers
   must be identical to before.

## Definition of Done

- `#buildOllyUsageData` reads from `response.date` + `response.userUsages`
  (non-deprecated fields).
- New `unified-data-usage.service.spec.ts` covers the four cases above and
  passes.
- `pnpm nx lint settings-core` and `pnpm nx test settings-core` are green.
- No changes outside `unified-data-usage.service.ts` and its new spec.
- PR description flags the intentional side-effect on the Quota Allocation
  Rules page (see Risk #2) so reviewers can confirm the intent.
