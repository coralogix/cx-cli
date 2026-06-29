## FORGE-32 — Unify Data Usage tabs (Olly + AI Center) into "AI Units"

### Scope & high-level approach

Frontend-only change inside `libs/settings/core/src/lib/data-usage/`. Replace the standalone **Olly** (`ollyUsage`) and **AI Center** (`aiEvals`) tabs with a single **AI Units** (`aiUnits`) tab that stacks three collapsible sections (Total · Olly · AI Center). Keep `ollyUsage` and `aiEvals` as **internal sub-section scopes** so the existing widgets/graph/table/grouping/filter pipelines keep working unchanged for the two sub-sections. Add a new tab-level scope `aiUnits` plus a small aggregation utility for the Total section. Rename generic "Olly Units" / "Olly unit usage" labels on this screen to "AI Units" / "AI unit usage".

IBM is unaffected (its tabs list has neither today). Gov cloud already omits both — the new `aiUnits` tab must also be omitted in Gov (preserve the `isGov` gate).

### Run / verify commands

Project under change: `settings-core` (Nx project name; path `libs/settings/core`).

| What | Command |
|---|---|
| Unit tests | `pnpm nx test settings-core` |
| Lint | `pnpm nx lint settings-core` |
| TypeScript strict check | `pnpm nx tsc-strict settings-core` |
| Build | `pnpm nx build web-app` |
| Smoke test (real env) | `pnpm nx e2e web-app -- --project=smoke --reporter=list` (existing `data-usage.smoke.ts`) |
| Affected | `pnpm nx affected -t lint,test,build` |
| Dev server | `pnpm run dstaging` (requires staging credentials/network) |

> **Sandbox limitation surfaced**: this environment cannot run the app interactively against staging (no credentials, no browser session against `app.coralogix.com`). The before/after manual observation must be performed by the implementer locally with `pnpm run dstaging`, navigating to `settings/datausage`. The smoke test (`libs/settings/core/src/lib/data-usage/smoke-tests/data-usage.smoke.ts`) is the closest automated proxy; it asserts the User-Data tab loads and the export dialog opens — it does not click into the Olly/AI Center tabs today, so adding a smoke step that activates the **AI Units** tab and asserts the three section headers are visible is part of the work.

### Before-state to capture (manual)

Run `pnpm run dstaging`, sign in, navigate to **Settings → Data usage**, and take screenshots / a short Loom of:
1. Tab strip listing six tabs (`User data sent`, `System data sent`, `AI Center`, `Olly`, `Infrastructure data`, `Quota units`).
2. The **Olly** tab: free-tier banner (if non-paying), monthly-limit card, user/plan/model/token-type widgets and table.
3. The **AI Center** tab: policy/feature breakdown widgets and table.

After the change, capture the same screen with the single **AI Units** tab expanded showing Total + Olly + AI Center sections.

Save artifacts to `.saga/artifacts/`.

---

### Design decisions (chosen defaults; flag if implementation discovers a better fit)

These were offered to the user as clarifying questions; the user declined to answer. The implementer may revisit any of the three during the work if blocked:

1. **Total section visualization**: stacked column chart with two series (`Olly` and `AI Center`), sharing one summary-cards row (Total / Max / Min / Avg AI Units). Easiest reuse of existing `UsageGraphComponent` + `series.utils.ts`.
2. **Filter / Group-By toolbar on AI Units tab**: per-section toolbars — Olly and AI Center sub-sections each render their own filter + group-by toolbar (the existing one, scoped to that section's data). The Total section has **no** filter/group-by toolbar (its chart is just the aggregated daily totals). Date range and aggregation buttons stay global at the page level.
3. **Renaming scope**: rename only the generic "Olly Units / Olly unit usage" tooltips and tab title. Keep Olly-product-specific strings (free-tier banner copy, `MONTHLY_LIMIT_TOOLTIP` describing Olly's allocation, `OLLY_DAILY_USAGE` series name) untouched — they remain factually about the Olly product, only the *unit billing* terminology becomes "AI Units".

---

### Order of changes (dependencies first)

#### Step 1 — Types & scope union

**`libs/settings/core/src/lib/data-usage/data-usage.types.ts`**

- Add `aiUnits` as a new `UsageData` variant. It is a tab-level composite that, in practice, only ever holds the *aggregated Total daily totals* (the Olly and AI Center sub-sections still use the existing `ollyUsage` / `aiEvals` variants internally):
  ```ts
  | { $case: 'aiUnits'; data: DetailedDailyDataMapping[] }
  ```
  This is the minimal addition that lets `getChartSeries`, `getUsageTotal`, the card builder, and the graph component handle the Total section without a third codepath.
- `DataUsageScope` is derived from `UsageData['$case']`, so it gains `'aiUnits'` automatically.

#### Step 2 — Tab list & gov gating

**`libs/settings/core/src/lib/data-usage/data-usage.config.ts`**

- `DataUsageTranslations.TABS`: drop `AI_EVALUATORS` and `OLLY_USAGE`; add `AI_UNITS`.
- `createDataUsageTabs()`:
  - Replace both tab objects (currently lines ~57–66) with a single entry:
    ```ts
    {
      alias: 'aiUnits' as const,
      title: translations.TABS.AI_UNITS,
      icon: 'general/ai.svg' as const,
    }
    ```
  - Keep the `isGov` guard that omits this branch entirely on Gov1.
- `getGroupingOptionsForScope()`: add an `aiUnits: []` entry (empty — the AI Units tab itself has no global Group By; per-section group-by options reuse the existing `ollyUsage` and `aiEvals` entries which stay in this map).

**`libs/settings/core/src/lib/data-usage/data-usage.config.spec.ts`**

- Update `TRANSLATIONS.TABS` (drop `AI_EVALUATORS`/`OLLY_USAGE`, add `AI_UNITS: 'AI Units'`).
- Update the two `createDataUsageTabs` assertions:
  - non-IBM, non-Gov: `['userData', 'systemData', 'aiUnits', 'eventsSent', 'units']`.
  - Gov: unchanged (`['userData', 'systemData', 'eventsSent', 'units']`).
- Add a `getGroupingOptionsForScope('aiUnits', ...)` case returning `[]`.
- The existing `'ollyUsage'` and `'aiEvals'` getGroupingOptionsForScope tests stay — those scopes are still used internally for sub-sections.

#### Step 3 — Scope-filter & utility plumbing

**`libs/settings/core/src/lib/data-usage/utils/scope-filter.utils.ts`**

- Add `case 'aiUnits': return false;` (mirrors `ollyUsage` — the AI Units tab does its own aggregation and doesn't go through the generic flat-data → scope-filter path).
- Update `scope-filter.utils.spec.ts` exhaustiveness test if any.

**`libs/settings/core/src/lib/data-usage/utils/axis.utils.ts`**

- Add `aiUnits: translations.UNITS` to the `getSecondaryYAxisUnit()` units map.

**`libs/settings/core/src/lib/data-usage/utils/series.utils.ts`**

- The existing `else` branch already handles `DetailedDailyDataMapping`-shaped entries; `aiUnits` data is shaped that way, so the chart will render two stacked series by `dataItem.name` (`Olly`, `AI Center`) by default. The special-case at line 91 (`$case === 'ollyUsage' && name === 'Total'`) does not match the new shape and needs no change.
- Confirm `getLineSeries` `yAxisIndex` treatment is acceptable for `aiUnits` (probably keep on left axis 0 like ollyUsage by including `'aiUnits'` in the `scope === 'units' || scope === 'ollyUsage' || ...` branch at line 160). Decide based on whether the Total chart shows a quota line (it doesn't, since there's no AI Units quota yet — leave on default axis 0).

**`libs/settings/core/src/lib/data-usage/services/data-usage-grouping.service.ts`**

- Add `'aiUnits'` to `getDefaultGroupingForScope()` returning `{ criteria: [], enabled: false }` (the Total section is ungrouped).
- The `applyGrouping()` switch is keyed off scope and ultimately reads from `filterDailyUsageByScope` which now returns `[]` for `aiUnits`. The Total section never calls `applyGrouping` (it uses the dedicated aggregator), so no other change here.

**`libs/settings/core/src/lib/data-usage/services/data-usage-filtering.service.ts`**

- Add `case 'aiUnits': return [];` to `#getBaseFilterKeysForScope()` (no filters at the Total level).

**`libs/settings/core/src/lib/data-usage/services/data-usage-raw-data.service.ts`**

- Add `aiUnits: () => false` to the scope-predicate maps (lines ~28, 158, 170 area) and `aiUnits: 0` to the value-by-scope map (line ~228). The Total section doesn't go through `convertRawDataToUsageData`, so these are exhaustive-switch hygiene only.

#### Step 4 — New Total aggregation utility

Create **`libs/settings/core/src/lib/data-usage/utils/ai-units-total.utils.ts`** with:

```ts
import type { UserDateFormat } from '@cx/core';
import type { DataUsageItem, DetailedDailyDataMapping, OllyUsageData, UsageData } from '../data-usage.types';
import type { FlatDailyUsage } from '../models/grouping.types';
import { formatDate, generateNDayCategories, parseDateFromFormat, toIsoDateKey, type AggregationMode } from './date.utils';

const SERIES_OLLY = 'Olly';
const SERIES_AI_CENTER = 'AI Center';

interface BuildOptions {
  ollyData: OllyUsageData | null;
  flatData: FlatDailyUsage[];
  aggregationMode: AggregationMode;
  dateType: UserDateFormat;
  rangeStartDate?: Date;
  rangeEndDate?: Date;
}

/** Per-day map of AI-evaluation units extracted from FlatDailyUsage. */
function extractAiEvalsDailyUnits(flatData: FlatDailyUsage[]): Map<string, number> {
  const result = new Map<string, number>();
  for (const day of flatData) {
    let units = 0;
    for (const point of day.dataPoints) {
      if (point.measurementSource === 'aiEvaluation') units += point.units;
    }
    result.set(toIsoDateKey(day.date), units);
  }
  return result;
}

/**
 * Returns a UsageData with $case: 'aiUnits' whose `data[]` has one entry per
 * day/bucket and two series per entry (`Olly`, `AI Center`). Total/Cards
 * pipelines read `entry.total.value` as the combined AI Units.
 */
export function aggregateAiUnitsTotal(opts: BuildOptions): UsageData { /* ... */ }
```

The implementation:

1. **Pick the day axis** from the wider of `ollyData.days` ∪ `flatData.date`. (In practice `flatData` is the bucketed daily-usage list and is the authoritative range.)
2. For **daily** mode, walk each day and emit a `DetailedDailyDataMapping` with:
   ```ts
   {
     statsDate: { name: 'Stats date', value: day },
     total: { name: 'Total', value: ollyUnits + aiEvalsUnits, units: ollyUnits + aiEvalsUnits },
     data: [
       { name: 'Olly', value: ollyUnits, units: ollyUnits },
       { name: 'AI Center', value: aiEvalsUnits, units: aiEvalsUnits },
     ],
   }
   ```
3. For **weekly / monthly** modes, reuse the same bucket-key strategy as `aggregateOllyUsageToUsageData` (generate categories via `generateNDayCategories` or `formatDate(... 'monthly')`, sum each day's `ollyUnits + aiEvalsUnits` into the bucket per series). The output is a `UsageData` with `$case: 'aiUnits'`.
4. Return early with an empty `data: []` when both inputs are empty.

Add **`ai-units-total.utils.spec.ts`** covering:
- Daily mode with both sources contributing.
- Daily mode with one source empty (returns 0 for that series, total = other source).
- Weekly mode bucketing across a multi-week range.
- Date range with no Olly data (Gov-like).

Export from `utils/index.ts`.

#### Step 5 — Card builder for `aiUnits`

**`libs/settings/core/src/lib/data-usage/utils/card-builder.utils.ts`**

- `buildCardsForScope()` currently fans out to `buildQuotaBasedCards` (units/userData), `buildAiEvalsCards` (aiEvals), or `buildSimpleCards` (everything else, including ollyUsage). The `aiUnits` Total card row should be small (Total / Max daily / Min daily / Avg daily AI Units) — `buildSimpleCards`'s shape fits well.
- Add `aiUnits` to `formatValueForScope()`: `return { value: formatNumber(value), unit: translations.UNITS };` (label `units`).
- Route `aiUnits` through `buildSimpleCards` by default (no new branch needed if `formatValueForScope` returns the right unit) — verify by reading the function once.
- Update `card-builder.utils.spec.ts` with an `aiUnits` smoke case if other scopes are covered.

#### Step 6 — Component state & computed signals

**`libs/settings/core/src/lib/data-usage/data-usage.component.ts`** — the heavy lift.

Add new state for the per-section views inside the AI Units tab. Keep the existing single-tab state intact for all other tabs.

1. **Per-section filter & grouping state** (signals):
   ```ts
   readonly #ollySectionFilterConfig = signal<FilterConfig>({ filters: [], enabled: false });
   readonly #ollySectionGroupingConfig = signal<GroupingConfig>(
     this.#groupingService.getDefaultGroupingForScope('ollyUsage')
   );
   readonly #aiEvalsSectionFilterConfig = signal<FilterConfig>({ filters: [], enabled: false });
   readonly #aiEvalsSectionGroupingConfig = signal<GroupingConfig>(
     this.#groupingService.getDefaultGroupingForScope('aiEvals')
   );
   ```
   The existing `#filterConfig` / `#groupingConfig` continue to drive the other tabs and are unused by the AI Units tab.

2. **Per-section computed views**:
   - `protected readonly ollySectionFilterOptions = computed(() => this.#getOllyFilterOptions());`
   - `protected readonly aiEvalsSectionFilterOptions = computed(() => { /* same body as filterOptions() default branch, hard-coded to 'aiEvals' */ });`
   - `protected readonly ollySectionGroupingOptions = computed(() => getGroupingOptionsForScope('ollyUsage', ...));`
   - `protected readonly aiEvalsSectionGroupingOptions = computed(() => getGroupingOptionsForScope('aiEvals', ...));`
   - `protected readonly ollySectionDataUsage = computed<UsageData | null>(() => this.#computeOllyUsageData(this.#ollySectionFilterConfig(), this.#ollySectionGroupingConfig()))` — refactor `#computeOllyUsageData()` to accept (filterConfig, groupingConfig) explicitly instead of reading `this.#filterConfig()` / `this.#groupingConfig()` directly.
   - `protected readonly aiEvalsSectionDataUsage = computed<UsageData | null>(() => /* same pipeline as the default branch in dataUsage(), but with scope hard-coded to 'aiEvals' and filter/grouping from the aiEvals section signals */);`
   - `protected readonly aiEvalsSectionTableData = computed<UsageData | null>(() => /* same as tableData() default branch with scope='aiEvals' */);`
   - `protected readonly ollySectionUsageStats = computed(() => /* mirrors the ollyUsage branch in usageStats() */);`
   - `protected readonly aiEvalsSectionUsageStats = computed(() => /* mirrors the non-olly branch with scope='aiEvals' */);`
   - `protected readonly ollySectionCards = computed<UsageWidgetCard[]>(() => buildCardsForScope({ ..., scope: 'ollyUsage', ollyAllocation: this.#ollyAllocation() }));`
   - `protected readonly aiEvalsSectionCards = computed<UsageWidgetCard[]>(() => buildCardsForScope({ ..., scope: 'aiEvals', rawDailyUsage: /* aiEvals-scoped per-day usage from #rawUsageData filtered to aiEvaluation source */ }));`
   - `protected readonly totalSectionUsageData = computed<UsageData | null>(() => aggregateAiUnitsTotal({ ollyData: this.#ollyUsageData(), flatData: this.#rawUsageData()?.dailyUsage ?? [], aggregationMode: this.aggregationMode(), dateType: this.dateType(), rangeStartDate: this.rangeStartDate(), rangeEndDate: this.rangeEndDate() }));`
   - `protected readonly totalSectionUsageStats = computed(() => /* reduce totalSectionUsageData over per-bucket totals to get DailyStats */);`
   - `protected readonly totalSectionCards = computed<UsageWidgetCard[]>(() => buildCardsForScope({ usageStats: this.totalSectionUsageStats(), scope: 'aiUnits', aggregationMode: ..., translations: this.#cardTranslations(), formatBytes: ..., overviewMetrics: null }));`

3. **`#ollyAllocation()` already gates on `activeTabAlias() !== 'ollyUsage'`** (line 313). Update it to fire when the *Olly section* is being rendered, i.e. when `activeTabAlias() === 'aiUnits'` (the section is always rendered then).

4. **`ollyFreeTierAnnouncement()`** (line 750–768): change the gate from `activeTabAlias() !== 'ollyUsage'` to `activeTabAlias() !== 'aiUnits'`. The banner is anchored at the top of the **Olly sub-section** in the template, so it appears only when the AI Units tab is active and the team is non-paying.

5. **`graphCategories()`** (line 601) currently special-cases `scope === 'ollyUsage'`. The `aiUnits` path uses `generateGraphCategories` against the **range**, not against `#ollyUsageData`. Add an `'aiUnits'` branch that mirrors the ollyUsage branch (since the Total chart's x-axis is range-driven, not data-driven). Per-section charts inside the tab reuse the same categories.

6. **`#fetchAllData()` (line 1330)**: no changes to the request set — the existing parallel fetches already provide both `bucketsData` (containing aiEvaluation points) and `ollyUsage`. The Total/Olly/AI Center sections all derive from the same fetched data, so a single date-range or aggregation change still triggers one consolidated fetch that updates all three sections — satisfying acceptance criterion #6.

7. **`#handleTabChange()`** (line 1253): when switching *into* `aiUnits`, reset the two per-section filter/grouping signals to defaults (mirroring how the existing single-tab state resets on tab change).

8. **Existing `tableData` / `dataUsage` / `usageStats` / `usageWidgetCards` computed signals**: leave them branching on `activeTabAlias()`. When the active tab is `aiUnits`, they can return `null`/empty and the template doesn't render the existing single-section block — branch in the template instead.

9. **Refactor opportunity**: `#computeOllyUsageData()` (line 1183) and `#getOllyFilterOptions()` (line 1220) currently read instance signals. Parameterize them to take `(filterConfig, groupingConfig)` so they can be called both for the main signal path (where they read `this.#filterConfig()` / `this.#groupingConfig()` via a wrapper) and for the Olly sub-section signal path (where they read `this.#ollySectionFilterConfig()` / `this.#ollySectionGroupingConfig()`). This avoids logic duplication.

#### Step 7 — Template (data-usage.component.html)

Inside the `<ng-template cxuiTabContent>` block (line 91 onwards), branch on `tab.alias`:

- **Existing tabs** (`userData`, `systemData`, `eventsSent`, `metricsSamples`, `units`): unchanged.
- **`aiUnits` tab**: render three stacked `cxuiCollapsible` panels (Total, Olly, AI Center). Each panel:
  1. **Total**:
     - Header chip: `{{ translations.TOTAL_LABEL }}` (new i18n key, e.g. `AI_UNITS_TOTAL_SECTION_TITLE`).
     - Body: cards row (totalSectionCards) + graph (`<sh-usage-graph [scope]="'aiUnits'" [usageData]="totalSectionUsageData()" ...>`). No usage-table.
     - No per-section toolbar.
  2. **Olly**:
     - Header chip: `{{ translations.OLLY_SECTION_TITLE }}` (new key e.g. `Olly`).
     - Body: free-tier announcement (if applicable, gated on `ollyFreeTierAnnouncement()`); per-section toolbar (filter-builder bound to `ollySectionFilterOptions()` + flow-list bound to `ollySectionGroupingOptions()`); cards row (`ollySectionCards`); graph (`<sh-usage-graph [scope]="'ollyUsage'" [usageData]="ollySectionDataUsage()" ...>`); usage-table (`<sh-usage-table [scope]="'ollyUsage'" [ollyUsageData]="ollyUsageData()" ...>`).
  3. **AI Center**:
     - Header chip: `{{ translations.AI_CENTER_SECTION_TITLE }}`.
     - Body: per-section toolbar (filter+group bound to AI Center section signals); cards row (`aiEvalsSectionCards`); graph (`<sh-usage-graph [scope]="'aiEvals'" [usageData]="aiEvalsSectionDataUsage()" ...>`); usage-table (`<sh-usage-table [scope]="'aiEvals'" [dataUsage]="aiEvalsSectionTableData()" ...>`).
- Shared page-level toolbar elements that stay above the sections on the AI Units tab: **date-range picker** and **aggregation buttons**. The global filter/group-by toolbar block is **hidden** for `aiUnits` (filters live inside each section). The `Statistics` collapsible wrapper used by other tabs is replaced by the three per-section collapsibles.

The `configureBucketAnnouncements` block (line 92) is currently shown above the toolbar for all tabs. Keep it as-is for AI Units too (it's metrics-archive guidance, not Olly-specific).

Add new event handlers in the component for the per-section filter/grouping changes (parallel to `onFilterChange` / `onGroupingChange`).

#### Step 8 — Sub-components touched

**`libs/settings/core/src/lib/data-usage/components/usage-graph/usage-graph.component.ts`**

- Add `aiUnits` to `#graphHeadersMap`:
  ```ts
  aiUnits: {
    graphHeader: this.usageGraphTranslations.CARD_HEADER.AI_UNITS, // new i18n key
    tooltipHeader: this.usageGraphTranslations.TOOLTIP.UNITS_HEADER,
    unit: this.dataUsageTranslations.UNITS_UNIT,
  }
  ```
- The tooltip column-config switch (line 627) defaults `units` to a percent-of-quota secondary column — `aiUnits` has no quota, so add a dedicated branch that returns just the primary `UNITS_HEADER` with no secondary map (similar to `eventsSent` minus the secondary).

**`libs/settings/core/src/lib/data-usage/components/usage-table/usage-table.component.ts`**

- Add `aiUnits: this.tableTranslations.TITLE.TOTAL` to the `title` map (Total section has no table, but the union must be exhaustive).

**`libs/settings/core/src/lib/data-usage/components/usage-table/usage-table.service.ts`**

- `getFirstHeaderName()` / `getTotalColumnHeaderName()`: add `aiUnits` branches returning sensible defaults (unused at runtime but needed for exhaustiveness).

**`libs/settings/core/src/lib/data-usage/components/usage-table/usage-table-cell-content/usage-table-cell-content.component.ts`**

- Switch statements at lines ~116, 141, 143 cover all scopes — add `aiUnits` no-op branches as needed for exhaustiveness.

**`libs/settings/core/src/lib/data-usage/components/export-data-usage-dialog/export-data-usage-dialog.component.ts`**

- `availableTabs` (line ~198): the filter currently drops `ollyUsage` from the report-type tab list. With the new `aiUnits` tab now present in `availableTabs`, decide: should the export dialog expose `aiUnits` as a tab option? The proto enum `ReportTabs` has no `aiUnits` value — and the actual exportable AI data on the backend is still the `REPORT_TABS_AI_EVALUATIONS` proto. Cleanest: **filter `aiUnits` out of `availableTabs` like `ollyUsage` is filtered out today**. The dialog will list `userData`, `systemData`, `aiEvals` (still labeled "AI Center" internally; only the *tab* alias changed), `eventsSent`, `units` — same as today minus Olly. The default-selected tab when opening the dialog from the AI Units tab: pre-select `aiEvals` (drop the `[this.data.activeTab]` seeding and fall back to `userData` or the active tab if it's exportable; if it's `aiUnits`, swap to `aiEvals`).
- `applyFiltersAvailable` (line ~192): "Apply filters" only works when `tabs.length === 1 && tabs[0] === data.activeTab`. With `activeTab === 'aiUnits'`, this can never match an exportable scope, so the toggle is effectively always disabled on this tab — that's acceptable for v1 and aligns with the per-section filters not being a single-scope filter set.

**`libs/settings/core/src/lib/data-usage/services/report-export/report-export.tokens.ts`**

- No new entries needed for `aiUnits` (excluded from `ProtoBackedScope`). Leave `aiEvals` and `ollyUsage` entries; the latter is still referenced by `SCOPE_FILE_SEGMENT` if any code still needs to format an Olly file name (likely dead now, but safe to keep).

#### Step 9 — i18n (libs/i18n/cx/data-usage/en.json)

Apply the **conservative renaming** (chosen default — see Design decisions above):

- `MAIN.TABS.OLLY_USAGE` and `MAIN.TABS.AI_EVALUATORS`: remove both keys (no longer referenced).
- `MAIN.TABS.AI_UNITS: "AI Units"`: new key, used by the new tab.
- `MAIN.AI_UNITS_TOTAL_SECTION_TITLE: "Total"`, `MAIN.OLLY_SECTION_TITLE: "Olly"`, `MAIN.AI_CENTER_SECTION_TITLE: "AI Center"`: new keys for the three section headers inside the AI Units tab.
- `MAIN.OLLY.TOTAL_UNITS_TOOLTIP`: `"Total Olly units consumed during the selected timeframe"` → `"Total AI units consumed during the selected timeframe"`.
- `MAIN.OLLY.MAX_DAILY_TOOLTIP`: `"Highest daily Olly unit usage"` → `"Highest daily AI unit usage"`.
- `MAIN.OLLY.MIN_DAILY_TOOLTIP`: `"Lowest daily Olly unit usage"` → `"Lowest daily AI unit usage"`.
- `MAIN.OLLY.AVG_DAILY_TOOLTIP`: `"Average daily Olly unit usage"` → `"Average daily AI unit usage"`.
- `USAGE_WIDGET.TOOLTIPS.OLLY.TOTAL`: `"Total Olly units consumed during the selected timeframe"` → `"Total AI units consumed during the selected timeframe"`.
- `USAGE_WIDGET.TOOLTIPS.OLLY.DAILY_MAX`: `"Highest daily Olly unit usage"` → `"Highest daily AI unit usage"`.
- **Leave untouched** (Olly-product-specific, not unit terminology): `MAIN.OLLY.FREE_TIER_BANNER_MESSAGE` (Olly free tier is a product), `MAIN.OLLY.MONTHLY_LIMIT_TOOLTIP` (Olly's allocation specifically), `USAGE_GRAPH.CARD_HEADER.OLLY_USAGE` (used inside the Olly sub-section only — keep "Olly usage" wording), `DATA_USAGE_SERIES.OLLY_DAILY_USAGE` (series label inside Olly section).
- `USAGE_GRAPH.CARD_HEADER.AI_UNITS: "AI Units usage"`: new key for the Total section graph header.

IBM i18n (`libs/i18n/ibm/data-usage/en.json`): no changes — IBM tabs never include `aiUnits`/Olly/AI Center. **Do not touch IBM translations** per the workspace CLAUDE.md rule.

#### Step 10 — Smoke test update

**`libs/settings/core/src/lib/data-usage/smoke-tests/data-usage.smoke.ts`** + **`data-usage.driver.ts`**

- The current smoke spec only opens the default (User data) tab. Add a step that clicks the **AI Units** tab and asserts the three section headers are visible (`Total`, `Olly`, `AI Center`). Gate behind a `test.step` so existing pre-merge checks aren't blocked if the env's permission setup hides the AI tabs. Follow the anti-flakiness rules in `.claude/rules/smoke-tests-anti-flakiness.md` — don't assert on numeric usage values, only on presence of the section containers.

#### Step 11 — Unit tests to update / add

- `data-usage.config.spec.ts` — already covered in Step 2.
- `scope-filter.utils.spec.ts` — add an `aiUnits` case asserting `filterDailyUsageByScope` returns empty.
- `card-builder.utils.spec.ts` — add an `aiUnits` smoke case verifying the four cards.
- `ai-units-total.utils.spec.ts` — new (Step 4).
- `olly-usage-transformer.spec.ts` — no changes; Olly transformer still feeds the Olly sub-section.
- `data-usage-grouping.service.spec.ts` — add an `aiUnits` case for `getDefaultGroupingForScope`.
- `data-usage-filtering.service.spec.ts` — add an `aiUnits` case asserting empty filter keys.
- `data-usage-stats.service.spec.ts` — no changes (Total section uses bespoke aggregation, not this service).

---

### Edge cases & risks

- **Empty Olly / Empty aiEvals on the same date**: Total per-day = 0; the cards must show 0 not "—". Confirmed by `aggregateAiUnitsTotal` returning entries even for empty days (always emit a `data[]` with both series at 0).
- **Date range straddling Olly availability window**: `getOllyUsageByDateRange` may return fewer days than the bucketed `FlatDailyUsage` (Olly fetch is bounded by `#getOllyDateBoundaries`). The aggregator must walk **the bucketed date axis** (the bucketed-data is the authoritative range), filling Olly = 0 where the Olly response has no entry. Don't crash if `ollyData` is `null` (Gov fetch returns null today, but Gov also has no aiUnits tab — defensive null-handling stays in the utility).
- **Aggregation mode interaction**: bucket Olly per-day totals + aiEvals per-day units **before** bucketing into weekly/monthly. This keeps parity with how `aggregateOllyUsageToUsageData` and the existing daily-data pipeline already produce weekly/monthly buckets. The Total chart's effective aggregation mode is whatever the user picked — same as today.
- **`#ollyAllocation()` was previously gated on tab===ollyUsage**: now it fires whenever `aiUnits` is the active tab. Be sure the `listQuotaAllocationRules` call in `#fetchAllData` (line 1361) still runs in the new tab — it currently runs **unconditionally** for non-Gov, so OK.
- **Today's-only fetch (`#todayRawUsage`)** is only consumed by the `units`-scope "Current usage" card; it does not affect AI Units. Leave untouched.
- **PayG total cards**: only built for `units`/`userData` (in `buildQuotaBasedCards`). Not relevant to AI Units cards.
- **Color picker** (`resetColorAssignments`): currently reset on tab change and filter change. Reset it when toggling per-section filters too, so Olly and AI Center color palettes don't bleed.
- **Existing default tab** is `userData`. Don't change the default — the AI Units tab is just one of several tabs.
- **The `ollyUsage` and `aiEvals` aliases remain in `DataUsageScope`** because the sub-section components still use them. This is intentional and the ticket allows it ("two existing scopes may be retained internally for the sub-sections"). The user-visible *tab* is gone; the internal *scope* persists.
- **Backwards-compat for any consumers of `DataUsageScope`** outside `data-usage/`: search `apps/web-app` and `libs/` for `'ollyUsage'` / `'aiEvals'` as string literals (e.g. routing, links, analytics). None expected, but verify.

---

### Verification plan (after implementation)

1. **Unit & lint** — must all pass:
   ```
   pnpm nx test settings-core
   pnpm nx lint settings-core
   pnpm nx tsc-strict settings-core
   pnpm nx build web-app
   ```
2. **Manual** (local `pnpm run dstaging`, real staging backend):
   - The tab strip shows `User data sent`, `System data sent`, `AI Units`, `Infrastructure data`, `Quota units` (5 tabs, was 6).
   - Click **AI Units** → three sections appear in order Total / Olly / AI Center. All collapse/expand independently.
   - **Total**: chart has Olly + AI Center stacked series; cards show summed Total / Max / Min / Avg AI Units.
   - **Olly**: free-tier banner appears for non-paying teams; per-user/plan/model/token-type widgets identical to the old Olly tab.
   - **AI Center**: policy/feature widgets identical to the old AI Center tab.
   - Change the date-range picker once → all three sections re-fetch simultaneously and update (AC #6).
   - Change aggregation Daily↔Weekly↔Monthly → all three section graphs re-bucket.
   - Set a filter in the Olly sub-section → Olly cards/graph/table update; Total + AI Center sections stay untouched (independent per-section filtering).
   - Open Generate report dialog → tab options exclude `aiUnits` and Olly; default-selected tab is something exportable.
   - **Gov build**: confirm AI Units tab is absent (gate by `isGov` flag).
   - **IBM build**: no changes from today (only User data sent + Metric usage).
3. **Smoke test** (against staging — implementer should run locally):
   ```
   pnpm nx e2e web-app -- --project=smoke --reporter=list \
     --grep "Data Usage"
   ```
4. **Artifact capture**: before/after screenshots saved to `.saga/artifacts/` per the Saga conventions.