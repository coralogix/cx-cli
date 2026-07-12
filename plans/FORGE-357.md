# FORGE-357 — Align markdown table column sizing with standard width calculation

## Summary of the change

Replace the "last-column-stretch" Olly override that gave the last markdown-table column all remaining horizontal space with a **content-weighted flex** layout applied uniformly in the base `CxuiMdTable` component. Each column gets a flex weight proportional to the longest text in that column (header + rows), so leftover container width is distributed across columns in proportion to their content — matching how ChatGPT and other standard GFM renderers lay tables out.

The ticket explicitly permits changing the base defaults *provided* the Olly-specific override is removed entirely (see "Out of scope" note). That is the path this plan takes: it collapses two overlapping sizing strategies into one shared implementation.

## Files touched

1. **`libs/cx-ui/markdown/src/lib/markdown-table.ts`** — replace uniform `flex: 1` with content-weighted `flex` in the `gridOptions` computed.
2. **`libs/olly/src/lib/provide-olly-services.ts`** — remove the entire `provideMdTableOptions(...)` block and its import.
3. **`libs/cx-ui/markdown/src/lib/markdown.vitest.ts`** — add a regression test asserting the proportional-width behavior.

`libs/cx-ui/markdown/src/index.ts` and the `CXUI_MD_TABLE_OPTIONS` / `provideMdTableOptions` public API stay as-is — the hook remains available for future consumers, it's just not used by Olly anymore.

## Current behavior (before-state)

The FORGE-357 bug is only observable end-to-end in Olly chat responses driven by the real AI backend (a markdown table with a long-content column that is *not* the last). Reproducing that in this worktree would require live backend/AI access, which is not available. Reproduction was therefore not attempted; the plan instead depends on:

- The bug is documented with a screenshot reference in FORGE-357 and its predecessor FORGE-304.
- The offending code path is small, isolated, and read directly (`libs/olly/src/lib/provide-olly-services.ts:59-78`) — it only ever sets `flex` on `columnDefs[lastIndex]` and only removes `maxWidth` there.
- New behavior will be pinned by a component test (see verification step below), which makes the regression trap explicit even without live reproduction.

Anyone wanting to sanity-check the base component's current uniform-flex behavior locally can do so via Storybook (see verification section) — that renders `CxuiMdTable` without the Olly override.

## Implementation

### 1. `libs/cx-ui/markdown/src/lib/markdown-table.ts` — content-weighted flex in the base component

Add a small helper (top-level, above the component, next to the other pure helpers) that measures a per-column content weight from a `Table` mdast node:

```ts
/**
 * Per-column weight = the longest text length across the header cell and every body cell
 * in that column. Used as an AG Grid `flex` value so leftover width is distributed
 * proportionally to content, matching how standard markdown renderers (e.g. ChatGPT)
 * lay tables out.
 *
 * Weight is floored at 1 so a fully-empty column doesn't collapse and doesn't create
 * a divide-by-zero when normalised.
 */
function computeColumnWeights(table: Table): number[] {
  const headerRow = table.children[0];
  if (!headerRow) return [];

  const colCount = headerRow.children.length;
  const weights = new Array<number>(colCount).fill(1);

  const measure = (row: Table['children'][number]) => {
    row.children.forEach((cell, i) => {
      if (i >= colCount) return;
      const len = extractText(cell.children as PhrasingContent[]).length;
      if (len > weights[i]!) weights[i] = len;
    });
  };

  measure(headerRow);
  for (const row of table.children.slice(1)) measure(row);

  return weights;
}
```

Then rewrite the `gridOptions` computed so per-column `flex` comes from the weights, not the current uniform `flex: 1`:

```ts
protected readonly gridOptions = computed<CXGridOptions>(() => {
  const key = this.#headerKey();
  if (!key) return { columnDefs: [] } as CXGridOptions;

  // Weights depend on node() (both headers and body rows). Streaming ticks
  // that only append rows will re-run this computed and re-flex the columns
  // — that is intentional: it matches how ChatGPT settles column widths as
  // content arrives.
  const weights = computeColumnWeights(this.node());
  const entries = key.split('\t');

  const columnDefs = entries.map((entry, index) => {
    const [headerName, align] = entry.split(HEADER_KEY_SEPARATOR);
    return {
      headerName,
      field: `col${index}`,
      cellRenderer: MdTableCellRenderer,
      cellStyle: { 'text-align': align },
      wrapText: true,
      autoHeight: true,
      maxWidth: MAX_COL_WIDTH,
      flex: weights[index] ?? 1,   // <-- was: flex: 1
      sortable: true,
      sortingOrder: ['asc', 'desc', null],
      comparator: phrasingComparator,
      filter: false,
      suppressMovable: true,
    };
  });

  const baseOptions = {
    columnDefs,
    domLayout: 'autoHeight',
    autoSizeStrategy: this.#autoSizeStrategy,
    getRowId: (params) => params.data.__id,
  } as CXGridOptions;

  return this.#customizeOptions(baseOptions);
});
```

Update the JSDoc comment immediately above `gridOptions` (currently at `markdown-table.ts:300-303` and 320-322) to reflect that the computed now depends on `node()` as well as `#headerKey`, because content weights need row data. Explicitly note that the streaming-tick recomputation is intentional and is how proportional widths settle as rows arrive.

### 2. `libs/olly/src/lib/provide-olly-services.ts` — remove the last-column-stretch override

Delete lines 59–78 in full (the comment plus the entire `provideMdTableOptions(...)` provider entry). Also remove the now-unused import:

```diff
-import { provideMdTableOptions } from '@cx/ui-markdown';
```

Leave the rest of the provider list untouched. This satisfies success criterion 1 (the specific override is removed/replaced), success criterion 2 (leftover width is now shared across all content-heavy columns via the base implementation), and preserves success criterion 3 (FORGE-304 full-width span is preserved because every column still has `flex > 0` and ag-grid stretches columns to fill the container).

### 3. `libs/cx-ui/markdown/src/lib/markdown.vitest.ts` — regression test for proportional widths

The existing `LongContentTableMarkdown` fixture (module-scope in the vitest file) already renders a 400px-wide 2-column table where column A ("Question") is short and column B ("Answer") is long. It's currently only used for a wrapping/height assertion. Add a new **assertion** test alongside those, inside the `describe('assertion', ...)` block:

```ts
test(`GIVEN a table with a short-content column and a long-content column
      WHEN the markdown renders
      THEN the long-content column is measurably wider than the short one`, async ({ render }) => {
  const { locator } = await render(LongContentTableMarkdown, {
    providers: providers(),
  });
  await expect
    .poll(() => locator.locator('.ag-header-cell').elements().length)
    .toBe(2);

  const headers = locator.locator('.ag-header-cell').elements();
  const shortWidth = headers[0].getBoundingClientRect().width;
  const longWidth = headers[1].getBoundingClientRect().width;

  // Proportional (content-weighted) flex should give the long-content
  // column visibly more space — not the ~50/50 split that uniform flex:1
  // would produce. Loose factor to survive small pixel differences.
  expect(longWidth).toBeGreaterThan(shortWidth * 1.5);
});
```

This test is the codified "before/after" boundary — it fails under the pre-change uniform-flex behavior (roughly equal columns) and passes after the change.

Do **not** relax or delete the existing `CXUI_MD_TABLE_AUTO_SIZE_STRATEGY: undefined` provider override at `markdown.vitest.ts:47`. That override is what makes columns fall back to their deterministic `flex` values in tests; the new proportional-flex behavior is exactly the fallback that this override exposes, so it should stay.

No new test file needs to be created in `libs/olly/`. There is no existing spec for the removed override, and the ticket's expected behavior is now covered at the component-library level where the logic actually lives.

## Order of changes (dependency order)

1. Edit `markdown-table.ts` — introduces the new base behavior. This alone changes the layout for **all** consumers, including Olly, but the Olly override still runs on top and short-circuits the last column. That's a temporarily inconsistent state; do not commit here.
2. Edit `provide-olly-services.ts` — removes the override so Olly now inherits the new base behavior.
3. Edit `markdown.vitest.ts` — locks the new behavior in.
4. Run the checks in "Verification".

## Edge cases & risks

- **Empty column (every cell blank in that column):** the weight is floored at 1, so ag-grid still assigns a small non-zero flex share; the column doesn't collapse.
- **Empty table (headers but zero body rows):** weights are computed from headers alone. Common during Olly streaming when the header row lands before the first body row — the initial layout uses header lengths, and rebalances once rows arrive.
- **Header row missing entirely:** `#headerKey` is empty → `gridOptions` returns `{ columnDefs: [] }` as it does today. No change here.
- **Extremely long content in one column:** `maxWidth: MAX_COL_WIDTH` (~400px) still caps the column. When one column caps, ag-grid redistributes remaining space to the other flex columns — this is standard ag-grid behavior and is what we want.
- **Wide container + few columns whose maxWidths don't reach the container width:** columns cap at `MAX_COL_WIDTH` and whitespace remains. This is the same soft edge case the base component already had before the Olly override was added; typical Olly chat containers (~672px `max-w-2xl`) don't reach the 800px+ threshold where this shows for 2–3 column tables, so it's an acceptable trade to keep the readability cap. Not in scope to change `MAX_COL_WIDTH`.
- **Streaming perf:** `gridOptions` now recomputes on every streaming tick (node() dependency). The work per tick is O(rows × cols) string-length reads on a small mdast tree, which is negligible for chat-sized tables. AG Grid preserves column state by `field` (`col0`, `col1`, …) across `columnDefs` updates, so sort / user-resize state is not reset by the re-flex. If perf ever surfaces as an issue, follow-up work can memoise weights via a string signature — not needed for this ticket.
- **`provideMdTableOptions` is now unused, but kept in the public API.** The hook itself was already exported before FORGE-304 as generic infrastructure. Leaving it exported preserves the option for future non-Olly customisation and avoids a pointless breaking change.

## Verification

**Run commands (from `frontend/`):**

```bash
# Type-check the affected libs
pnpm nx tsc-strict cx-ui-markdown
pnpm nx tsc-strict olly

# Lint the affected libs
pnpm nx lint cx-ui-markdown
pnpm nx lint olly

# Full component-test suite for markdown (must include the new assertion)
NX_TUI=false pnpm nx vitest-components cx-ui-markdown -- --reporter=list
```

**Behavior to observe:**

- Before change (baseline, on this branch's HEAD before edits):
  - `cx-ui-markdown` component tests all pass, but the new proportional-width test does not yet exist.
  - In Storybook (`pnpm run storybook-cxui`, `CXUI/Data Display/Markdown` → `Static` story): the "Feature | Signals | RxJS | NgRx" table renders with roughly equal columns (uniform flex:1). This is the base behavior — the Olly last-column stretch is *not* visible here because the story doesn't wire `provideOllyServices`.
- After change:
  - The new component test passes: for the 2-column `LongContentTableMarkdown`, the second column measures visibly wider than the first (≥1.5×).
  - All previously-existing assertion/visual tests in `markdown.vitest.ts` still pass (copy-as-CSV/JSON/Markdown, sorting, wrap/autoHeight, hover reveal, empty content, streaming, aria-live, aria-busy).
  - Storybook `Static` story now renders the 4-column comparison table with the wider columns going to whichever headers/cells have the longest text (visually inspectable). It should still fully span the container without whitespace.

**End-to-end (Olly) sanity check** — out of scope to automate in this worktree because it needs a live AI backend, but reviewers with Olly access should:
1. Send an Olly prompt that produces a 3+ column markdown table where an earlier (not last) column has the longest content.
2. Confirm that column — not the last — is now the widest.
3. Confirm the table still fills the width of the message container (no whitespace to the right of `ag-center-cols-container`).

**Visual snapshots:** the existing markdown-table screenshot tests (`renders table`, `renders table with copy button on hover`) will change under the new proportional-flex behavior. Do not hand-edit the committed `-chromium-linux.png` baselines. Once the PR is up, add the `update-snapshots` label so `cx-larry[bot]` regenerates them in CI (per `.claude/rules/cx-ui.md`).
