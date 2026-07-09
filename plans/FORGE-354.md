# FORGE-354 — Chart looks too small in artifact drawer — Technical Plan

## Summary
Five of the six chart-artifact-view components render at a hardcoded `tw-h-64` (256px), independent of the surrounding container. When rendered inside the artifact drawer (`cx-olly-artifact-drawer-card`, wider and full-viewport-tall), the chart appears visually undersized. This is the reported bug. Only `HorizontalBarChartArtifactView` currently sizes itself dynamically (`heightPx` signal driven by entry count) and is out of scope.

Fix: propagate the existing `isCollapsible` signal from `ChartPreviewComponent` down to each of the five affected chart views, and switch their fixed `tw-h-64` for a context-aware height that grows to ~320px when `isCollapsible === false` (i.e. drawer context) while preserving the current 256px in the collapsible inline-chat context. Match the loading skeleton in `chart-preview.component.html` so there is no layout shift.

This is a small, tightly scoped visual fix — no data-flow, service, or type changes.

## Environment / how to run

From `frontend/CLAUDE.md`:
- **Serve app**: `pnpm run dstaging` (dev with staging backend) or `pnpm nx serve web-app`
- **Lint the affected lib**: `pnpm nx lint olly`
- **Type-check the affected lib**: `pnpm nx run olly:tsc-strict`
- **Unit tests**: `pnpm nx affected -t test` — note `olly`'s `test` target is `nx:noop`, so there are no unit tests to run for this lib itself, but any downstream lib affected by the change will be picked up.

**Blocker for local repro in this worktree:** `frontend/node_modules` is not installed here, so I could not launch the dev server to visually reproduce the before-state during planning. The bug is well-established by the linked screenshot in the ticket and by directly reading the five components (all five render `<cxui-chart ... class="tw-h-64 tw-w-full" />`). The implementation step must:
1. `pnpm install` in `frontend/`.
2. `pnpm run dstaging` (or equivalent).
3. Open a chat that produces a chart artifact of each of the five types (line, area, pie, vertical-bar, vertical-stacked-bar), open it in the artifact drawer, and capture screenshots before and after the change. Save to `.saga/artifacts/` (naming per Saga convention).

## Files to change

Five chart-view components (add `isCollapsible` input, replace hardcoded `tw-h-64` with a context-aware height):

1. `frontend/libs/olly/src/lib/components/chart-preview/components/line-chart-artifact-view.component.ts`
2. `frontend/libs/olly/src/lib/components/chart-preview/components/area-chart-artifact-view.component.ts`
3. `frontend/libs/olly/src/lib/components/chart-preview/components/pie-chart-artifact-view.component.ts`
4. `frontend/libs/olly/src/lib/components/chart-preview/components/vertical-bar-chart-artifact-view.component.ts`
5. `frontend/libs/olly/src/lib/components/chart-preview/components/vertical-stacked-bar-chart-artifact-view.component.ts`

Parent template (forward the input and match the loading skeleton):

6. `frontend/libs/olly/src/lib/components/chart-preview/chart-preview.component.html`

Deliberately **not** changed:
- `horizontal-bar-chart-artifact-view.component.ts` — already dynamic; ticket explicitly out of scope. Its consumption in `chart-preview.component.html` already receives `isCollapsible` (via `maxEntries`) — no further changes needed.
- `base-chart-artifact-view.component.ts` — could host `isCollapsible` centrally, but keeping the input on the leaf components avoids coupling and mirrors how horizontal-bar keeps its `heightPx` local. (Decision: **keep it on the leaves**; horizontal-bar's overridden `heightPx` signal would otherwise fight a base-class version.)
- `metrics-artifact-bar-chart.component.ts` / `metrics-artifact-line-chart.component.ts` — also use `tw-h-64`, but they live in a different feature (`metrics-artifact-preview`) and are **not** listed in the ticket's success criteria. Leave for a separate ticket.
- `artifact-card.component.html:93` — a spinner placeholder inside a `loading` state fallback slot; unused when `chart-preview` supplies its own `ollyArtifactCardLoadingBody` slot (which it does). Not related to chart sizing.

## Implementation

### Step 1 — Add `isCollapsible` input + context-aware height to each of the five components

Add two shared constants (either inline in each file, or export from a small local module — recommend inline constants at the top of each file, matching how `horizontal-bar-chart-artifact-view.component.ts` inlines `ENTRY_HEIGHT_PX` / `DEFAULT_HEIGHT_PX`):

```ts
const COLLAPSED_HEIGHT_PX = 256; // matches previous tw-h-64 (chat-inline)
const EXPANDED_HEIGHT_PX  = 320; // matches horizontal-bar DEFAULT_HEIGHT_PX
```

Pattern (using `line-chart-artifact-view.component.ts` as the exemplar; apply the same shape to the other four):

```ts
import {
  ChangeDetectionStrategy,
  Component,
  booleanAttribute,
  computed,
  input,
} from '@angular/core';
// ... existing imports

const COLLAPSED_HEIGHT_PX = 256;
const EXPANDED_HEIGHT_PX = 320;

@Component({
  selector: 'cx-line-chart-artifact-view',
  template:
    '<cxui-chart [cxuiChartResource]="chartResource" class="tw-w-full" [style.height.px]="heightPx()" />',
  imports: [CxuiChart, CxuiChartResource],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LineChartArtifactViewComponent extends BaseChartArtifactViewComponent<LineChartDatetimeFloat> {
  readonly isCollapsible = input(true, { transform: booleanAttribute });

  protected readonly heightPx = computed(() =>
    this.isCollapsible() ? COLLAPSED_HEIGHT_PX : EXPANDED_HEIGHT_PX,
  );

  protected override buildChartConfig(view: LineChartDatetimeFloat): CxuiChartConfig {
    // ...unchanged
  }
}
```

Notes:
- Default is `true` so nothing else in the app that constructs one of these components directly regresses.
- Use `[style.height.px]` binding to mirror the exact pattern in `horizontal-bar-chart-artifact-view.component.ts:33`.
- `booleanAttribute` transform matches how `chart-preview.component.ts:51` declares its own `isCollapsible` input (keeps the API consistent when Angular attribute-binds `false`/`true` strings).
- `computed` (not `signal.set`) is the right primitive here: `heightPx` derives from `isCollapsible` — see `angular-21.md` rules ("`computed` for Derived State").
- Do the same for pie (`class="tw-w-full"` on `<cxui-chart>` — pie doesn't have a horizontal series, but the fixed height still applies).

Apply this same three-line change (input + computed + template binding) to all five files. No other changes to any `buildChartConfig()` implementation.

### Step 2 — Forward `isCollapsible` from `ChartPreviewComponent` to the five child views

Update `frontend/libs/olly/src/lib/components/chart-preview/chart-preview.component.html`:

- On lines 20, 23, 26, 35, 38, add `[isCollapsible]="isCollapsible()"`. `ChartPreviewComponent` already has `readonly isCollapsible = input(true, { transform: booleanAttribute });` (line 51 of the .ts) — no change needed on the parent class.
- Leave the `cx-horizontal-bar-chart-artifact-view` element unchanged (its sizing is already dynamic and correct; adding the input would be a no-op).

### Step 3 — Match the loading-skeleton height

Line 15 of `chart-preview.component.html`:

```html
<cxui-chart ollyArtifactCardLoadingBody loading class="tw-h-64" />
```

Change the class to a conditional height that mirrors the loaded chart's height, so switching from skeleton to real chart does not visually jump:

```html
<cxui-chart
  ollyArtifactCardLoadingBody
  loading
  [class.tw-h-64]="isCollapsible()"
  [class.tw-h-80]="!isCollapsible()"
/>
```

Tailwind values: `tw-h-64` = 256px (matches `COLLAPSED_HEIGHT_PX`); `tw-h-80` = 320px (matches `EXPANDED_HEIGHT_PX`). If those Tailwind classes are not present in the workspace preset, fall back to `[style.height.px]="isCollapsible() ? 256 : 320"` — but the classes are standard Tailwind defaults and should be available (verified in-line: `tw-h-64` is used elsewhere; `tw-h-80` is 20rem/320px in default Tailwind).

## Order of changes

1. Edit each of the five component `.ts` files (Step 1). They are independent — order does not matter.
2. Edit `chart-preview.component.html` to forward `isCollapsible` to each child and update the skeleton (Step 2 + Step 3).
3. Lint / type-check the `olly` lib.
4. Manually verify in the running app.

Step 1 must land before Step 2 because otherwise the template will bind an unknown input on the child components and Angular's strict template check will error.

## Edge cases / risks

- **Layout shift on load**: Addressed by Step 3 matching the skeleton to the loaded height.
- **`cx-olly-artifact-drawer-card` clipping**: The card wrapper (`artifact-drawer-card.component.html:5`) uses `tw-overflow-hidden` on its outer container but the content slot has no height constraint, and the drawer body (`artifact-drawer-content.component.html:73`) is `tw-flex tw-min-h-0 tw-flex-1 tw-flex-col tw-gap-4 tw-overflow-auto`, so a taller chart just makes the card taller and the drawer body scrolls. No clipping expected — but the implementer should scroll the drawer during verification to confirm.
- **`ChangeDetectionStrategy.OnPush`**: `isCollapsible` is a signal input, `heightPx` is a `computed` — both trigger CD correctly under OnPush. No additional plumbing.
- **Chat-inline path (`isCollapsible === true`)**: `heightPx` returns 256 → identical rendering to today (`tw-h-64` = 256px). Zero regression risk on the chat inline path.
- **Pie chart at 320px**: Pie charts scale to their container. Going from 256px to 320px will make the pie visibly larger in the drawer — this is the desired outcome, not a regression.
- **Horizontal-bar in the drawer**: Already dynamic — passes through, unchanged. Its default (5 rows × 64 = 320px) already matches our new drawer default; visual consistency preserved.
- **Height scales with drawer width?**: The ticket lists "scales with drawer width per `drawerWidth`" as an "and/or" — an alternative acceptance path. We are **not** implementing width-linked height in this pass. The fixed-larger drawer height satisfies the primary "chart height visibly increases in the artifact drawer" criterion and matches the horizontal-bar reference pattern the ticket calls out as canonical. If the reviewer prefers width-linked sizing, that becomes a follow-up (would require plumbing the drawer's `drawerWidth` signal into `ChartPreviewComponent`, or using a `ResizeObserver` in each chart view — non-trivial versus the value delivered).
- **Consumers outside `ChartPreviewComponent`**: `grep` shows the five chart-view components are only imported by `ChartPreviewComponent` (no other consumers). Adding an input with a default of `true` is fully backward compatible either way.

## Verification

Static checks (fast, in-worktree):
- `pnpm nx lint olly` — must pass.
- `pnpm nx run olly:tsc-strict` — must pass.
- `pnpm nx affected -t test` — nothing in `olly` runs, but neighbouring libs that transitively depend on `olly` may run. Must remain green.

Manual visual verification (required — this is a visual bug):
1. `pnpm run dstaging`.
2. Open an Olly chat that renders each chart type (or trigger via a test payload):
   - line chart
   - area chart
   - pie chart
   - vertical bar chart
   - vertical stacked bar chart
   - horizontal bar chart (regression check)
3. For each, capture the **inline chat card** — expected: unchanged (still 256px tall).
4. Click "Open in drawer" — expected: chart renders at 320px tall in the drawer (previously 256px). No layout shift from the loading skeleton to the loaded chart.
5. Resize the drawer with the drag handle — expected: no horizontal clipping; chart width tracks drawer width (already handled by `tw-w-full`). Chart height stays 320px (this pass does not link height to width).
6. Scroll the drawer body — expected: no clipping of the taller card inside `cx-olly-artifact-drawer-card`.
7. Save before/after screenshots to `.saga/artifacts/` (before: current master, after: with the fix). Name per Saga convention (`interface-type-*.png`).

Success criteria mapping (from the ticket):
- ✅ Non-horizontal-bar charts no longer hardcoded to `tw-h-64` — replaced by a context-aware `heightPx` binding.
- ✅ Chart height visibly increases in the artifact drawer (256px → 320px in the non-collapsible path).
- ✅ No regression to horizontal-bar (file untouched; parent template unchanged for that case).
- ✅ Existing component/unit tests pass — none exist for the five files (verified), so nothing to break; downstream `affected -t test` should stay green.
- ✅ No visual overflow/clipping in `cx-olly-artifact-drawer-card` — verified manually per Step 6 above.
