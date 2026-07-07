## Implementation plan — FORGE-275: Add "Ask Olly" button to cxui-page-header

### Goal
Move the projected `<cx-olly-ask-button class="tw-ml-auto" />` from the 12 pages that currently declare it as a direct child of `<cxui-page-header>` into `cxui-page-header`'s own template. Consumers stop projecting the button; the header renders it unconditionally. Standalone (non-header) usages are left untouched.

The button keeps its own `featureFlag`/consent gating internally, so `cxui-page-header` needs no new inputs. Per product decision (confirmed in the ticket), the button renders unconditionally — the 4 `cxui-page-header` consumers that don't have the button today (`service-dashboard/…/profiling-drilldown-dialog`, `pipeline-analyzer`, `home-dashboard/home-dashboard-page-header`, `continuous-profiling`) will gain it as an intended side effect.

### Run / check commands
From `frontend/`:
- Dev app: `pnpm run dstaging` (or `pnpm nx serve web-app`)
- cx-ui Storybook (fastest way to eyeball the header): `pnpm run storybook-cxui`
- Lint (affected): `pnpm nx affected -t lint`
- Tests (affected): `pnpm nx affected -t test`
- Component tests for the header (if added): `NX_TUI=false pnpm nx vitest-components cx-ui-page-header --run --browser.headless=true`
- Build: `pnpm nx build web-app`

### Observation strategy (before / after)

The plan step was executed in a sandbox with **no `node_modules` installed** — I could not run the app or storybook to capture the "before" state directly. This is a straightforward markup migration and the ticket description already enumerates the exact set of files and current-vs-desired behavior per file. The implementer should still capture artifacts:

1. **Before**: launch storybook (`pnpm run storybook-cxui`) — the Page Header story currently does **not** render the Ask Olly button. Launch web-app and visit any of the 4 pages listed above (e.g. `/pipeline-analyzer`) — no Ask Olly button in the header today. Screenshot both.
2. **After**: same storybook story and same pages — Ask Olly button now visible (right-aligned) on every page that uses `<cxui-page-header>`. Screenshot both. Also verify that on migrated pages (e.g. `/rum`, `/apm`, custom dashboards) there is still exactly **one** button in the header (no duplicate from the header + a leftover projection).
3. Save screenshots under `.saga/artifacts/`.

### Files to change

#### 1. Add the built-in button to `cxui-page-header`

`libs/cx-ui/page-header/src/lib/page-header.ts`

- Add `OllyAskButtonComponent` to imports:
  ```ts
  import { OllyAskButtonComponent } from '@cx/olly';
  ```
- Add `imports: [OllyAskButtonComponent]` on the `@Component` decorator (the component is currently `imports`-less because the template has no components).
- Append the button to the template, after `<ng-content />`:
  ```ts
  template: `
    <h1 class="tw-m-0 tw-mr-2 tw-text-text-primary tw-font-page-title">
      {{ pageTitle() }}
    </h1>

    <ng-content />

    <cx-olly-ask-button class="tw-ml-auto" />
  `,
  ```
  `tw-ml-auto` on the button's host works because the header is `tw-flex` — the auto left margin pushes it as far right as possible, exactly as it did when projected. The button's own `@if (isEnabled())` guard keeps the host empty when the `cx-olly` flag is off or consent is blocked in a way that hides it, so no layout regression on flag-off environments.

**Why this doesn't create a scope violation:** `cx-ui-page-header` is tagged `["cxui", "type:lib"]` and `olly` is `["type:lib"]`. `type:lib` is unconstrained (`onlyDependOnLibsWithTags: ['*']` in `eslint.config.mjs`), and `@cx/olly` does not import `@cx/ui-page-header`, so no circular dependency is introduced.

#### 2. Update the story (documentation only)

`libs/cx-ui/page-header/src/lib/page-header.stories.ts`

- Update the JSDoc `## Content Projection` section to mention that the Ask Olly button is now built in (no need to project it).
- No template change is strictly required — the button will render automatically inside the existing `Base` story once the feature flag is enabled (next step).

#### 3. Enable the `cx-olly` flag in the storybook

`apps/cx-ui-storybook/.storybook/storybook-feature-flags.ts`

Add `'cx-olly': true` so the Ask Olly button actually renders in the `Page Header` story (and any future header stories that embed it). Current file only enables the (now-defunct) `page-header-try-olly` flag.

```ts
export const cxuiStorybookFeatureFlags: Partial<Record<FeatureFlagsKey, boolean>> = {
  'page-header-try-olly': true,
  'cx-olly': true,
};
```

`OllyDrawerService`, `OllyAnalyticsService`, and `OllyConsentService` are all `providedIn: 'root'`, so the storybook `applicationConfig` does **not** need any new providers — the button will construct fine. Clicking it in storybook will call `OllyDrawerService.toggle()`, which will no-op safely because storybook is not on an Olly-eligible route.

#### 4. Remove the now-duplicate projection in 12 consumers

For each of the 12 files listed below, delete the `<cx-olly-ask-button ... />` element that sits as a **direct child** of `<cxui-page-header>`. Then in the co-located `.ts` file, delete `OllyAskButtonComponent` from the `imports` array and remove the `import { OllyAskButtonComponent } from '@cx/olly';` line **only if no other reference to it remains in that file**.

Migration list (verified via grep — all 12 use the default `featureFlag='cx-olly'`; none pass a custom flag):

| # | Template file | Companion `.ts` file |
|---|---|---|
| 1 | `libs/product-analytics/heatmaps/src/lib/heatmaps-header/heatmaps-header.component.html:6` | `heatmaps-header.component.ts` |
| 2 | `libs/rum/_shell/src/lib/rum-page/rum-page-header/rum-page-header.component.html:15` | `rum-page-header.component.ts` |
| 3 | `libs/apm-v2/src/lib/apm-header/apm-header.html:20` | `apm-header.ts` |
| 4 | `libs/explore/legacy/src/lib/explore.component.html:14` | `explore.component.ts` |
| 5 | `libs/explore/v2/src/lib/pages/logs-page/logs-page.component.html:27` | `logs-page.component.ts` |
| 6 | `libs/metric-explorer/src/lib/metric-explorer/metric-explorer.component.html:8` | `metric-explorer.component.ts` |
| 7 | `libs/ai-center/root/src/lib/cxai-center/cxai-center.component.html:3` | `cxai-center.component.ts` |
| 8 | `libs/ai-center/root/src/lib/cxai-center-application/cxai-center-application.component.html:8` | `cxai-center-application.component.ts` |
| 9 | `libs/dashboards/home-dashboard/_ui/src/lib/layout-components/dashboard-header/dashboard-header.component.html:2` | `dashboard-header.component.ts` |
| 10 | `libs/dashboards/visual-explorer/_ui/src/lib/visual-explorer-header/visual-explorer-header.component.html:2` | `visual-explorer-header.component.ts` |
| 11 | `libs/dashboards/custom-dashboards/header/_ui/src/lib/custom-dashboards-top-bar/custom-dashboards-top-bar.component.html:77` | `custom-dashboards-top-bar.component.ts` |
| 12 | `libs/xdr/cspm/src/lib/xdr.component.html:7` | `xdr.component.ts` |

For `visual-explorer-header.component.html`, the deletion leaves `<cxui-page-header ...>` with no projected content — convert the tag to a self-closing form (`<cxui-page-header ... />`) once the child is gone.

For `dashboards/home-dashboard/…/dashboard-header.component.html`, same thing — after deletion the header becomes childless; use self-closing.

For all other files, keep the surrounding markup exactly as-is; delete only the `<cx-olly-ask-button …>` line (and its surrounding whitespace if trivially trailing).

### Do NOT touch (regression guard)

These 9 usages are **standalone** — not inside `<cxui-page-header>` (they live inside plain `<div>`s, `<span>`s, dialogs, or a `<ng-template>`). They must remain unchanged. Verified by grep + context inspection during planning:

1. `libs/service-dashboard/src/lib/service-dashboard.component.html:74` (inside a `<div>`)
2. `libs/alerts/management/src/lib/alert-definition/alert-definition.component.html:38` (inside a `<div>`)
3. `libs/cases/src/lib/cases/pages/cases-listing/components/cases-top-bar/cases-top-bar.component.html:32` (inside a `<div>`)
4. `libs/cases/src/lib/cases/shared/components/case-details/case-details.component.html:11` (inside a dialog controls `<div>`)
5. `libs/notification-center/src/lib/notification-center.component.html:14` (inside a `<span>`)
6. `libs/tracing/src/lib/tracing-drilldown/components/tracing-drilldown-header/tracing-drilldown-header.component.html:17` (inside `<ng-template #exportActions>`; also uses a custom `featureFlag="tracing-drilldown-olly"`)
7. `libs/alerts-monitoring/src/lib/components/alerts-monitoring-header/alerts-monitoring-header.component.html:26` (sibling of a `<div>`, not inside `<cxui-page-header>`)
8. `libs/investigations/src/lib/pages/investigations-listing/components/investigations-top-bar/investigations-top-bar.component.html:5` (inside a `<div>`)
9. `apps/web-app/src/app/features/suppression-rules/suppression-rules.component.html:8` (inside a `<div>`)

### Verification test (recommended)

Add a Vitest isolated component test to guard against future regression. The page-header lib does not currently have any Vitest infra (no `vitest.config.mts`, no `tests-browser-setup.ts`, no `tsconfig.spec.json`). Setting it up is a small one-time cost — follow the checklist in `.claude/rules/vitest-component-testing.md` (§ Infrastructure Setup), then add:

`libs/cx-ui/page-header/src/lib/page-header.vitest.ts`

```ts
import { Component } from '@angular/core';
import { test, expect, describe } from '@cx/testing-vitest/isolated';
import {
  FeatureFlagsService,
  provideStaticFeatureFlags,
  StaticFeatureFlagService,
} from '@cx/feature-flags';

import { CxuiPageHeader } from './page-header';

@Component({
  imports: [CxuiPageHeader],
  template: `<cxui-page-header pageTitle="Test title" />`,
})
class Host {}

describe('CxuiPageHeader', () => {
  test(`GIVEN cx-olly flag enabled
        THEN the Ask Olly button is rendered without being projected`, async ({
    render,
  }) => {
    const screen = await render(Host, {
      providers: [
        { provide: FeatureFlagsService, useClass: StaticFeatureFlagService },
        provideStaticFeatureFlags({ 'cx-olly': true }),
      ],
    });

    await expect
      .element(screen.getByRole('button', { name: /ask olly/i }))
      .toBeVisible();
  });
});
```

Also add a `test: { executor: "nx:noop" }` target to `project.json` if the vitest plugin auto-infers a `test` target after the config file is added, per `.claude/rules/vitest-component-testing.md` (§ Step 6).

If setting up the vitest infra is deferred, at minimum update the story JSDoc so the change is documented — the ticket allows "updated stories **and/or** a Vitest component test" per its success criteria #1.

### Order of changes (dependencies first)

1. **Modify `page-header.ts`** — adds the button. On its own this creates a duplicate button (header renders it + each consumer still projects it), so all consumer edits should ship in the same commit/PR.
2. **Update storybook feature flags** — `cx-olly: true` — so storybook renders correctly during local dev.
3. **Update the story JSDoc** — documentation only.
4. **Delete 12 projections + prune 12 `.ts` imports** — in the same PR as step 1 to avoid the double-button window.
5. **(Recommended) Add Vitest infra + component test** — locks the invariant in.

### Edge cases & risks

- **Layout on migrated pages.** The button relies on `tw-ml-auto` to right-align inside the flex parent. Because it's now the last child of the template (positioned after `<ng-content />`), it stays the rightmost flex item. On the two pages where consumers pushed *other* elements right (`rum-page-header.component.html` currently has the button as the only right-aligned element; `custom-dashboards-top-bar` has the "All dashboards" button first and the ask-olly button with `tw-ml-auto` after it), the new placement matches the previous layout — the last flex item with `ml-auto` still ends up rightmost, and any earlier flex items stay in their natural left-to-right order.
- **Header height.** The header uses `tw-h-[42px]` and the Ask Olly button is `size="lg"`. Existing consumers already override with e.g. `class="tw-pb-4"` where needed. Not changing the header's own size — behavior remains identical for the 12 migrated pages.
- **4 new-button pages** (`pipeline-analyzer`, `profiling-drilldown-dialog`, legacy `home-dashboard-page-header`, `continuous-profiling`) will now show the Ask Olly button. This is intentional per the ticket. Two of these (`pipeline-analyzer`, `profiling-drilldown-dialog`) sit under routes that `OllyDrawerService.DISABLED_ROUTE_PREFIXES` excludes (`/pipeline-analyzer` is in the list; profiling dialogs live under `/settings`-adjacent routes — worth double-checking on the concrete route). On those routes, clicking the button will no-op (drawer refuses to open). The button will still be visible/enabled — call this out to the PR reviewer as an expected consequence of the unconditional-rendering decision. If product later wants to hide it on those pages, that would be a separate follow-up.
- **Consent-blocked state.** When `OllyConsentService.consentStatus() === 'blocked'`, the button renders but is `disabled` with a "admin required" tooltip. This is unchanged from the current per-page usage; it now applies to the 4 new-consumer pages too.
- **Standalone usage inside a lib that ALSO has a `cxui-page-header` in a different file.** No such case in the current codebase — grep confirmed none of the 21 `cx-olly-ask-button` usages sit inside `<cxui-page-header>` and also outside it in the same file.
- **Consumer with the button as the ONLY projected child** (e.g. `visual-explorer-header`, `home-dashboard/…/dashboard-header`). After the edit, `<cxui-page-header>` has no children — convert to self-closing form to keep the templates tidy. Not functionally required but cleaner.

### How to verify correctness after implementing

1. `grep -rn "<cx-olly-ask-button" --include="*.html"` — should show exactly the 9 standalone (out-of-scope) usages listed above. No result should be inside a `<cxui-page-header>...</cxui-page-header>` block.
2. `grep -rln "<cxui-page-header" --include="*.html"` — should still show all 16 consumers.
3. `pnpm nx affected -t lint` and `pnpm nx affected -t test` — pass. The `imports`-array pruning is caught by lint (`@nx/enforce-module-boundaries` + `unused-imports`); a missed removal will fail lint.
4. `pnpm nx build web-app` — pass. Standalone-component `imports` are validated at build time.
5. `pnpm run storybook-cxui` — the `CXUI/Layout & Navigation/Page Header/Base` story now shows the Ask Olly button, right-aligned, next to the existing projected content.
6. If Vitest test added: `NX_TUI=false pnpm nx vitest-components cx-ui-page-header --run --browser.headless=true` — passes; the test finds the button without any projection in the host template.
7. Manual smoke on `pnpm run dstaging`:
   - Visit a **migrated** page (e.g. RUM, APM, Metric Explorer, Custom Dashboards) — exactly one Ask Olly button in the header, right-aligned.
   - Visit a **standalone** page (e.g. Alerts management, Investigations listing, Notification Center) — Ask Olly button still in its original hand-rolled `<div>` location, unchanged.
   - Visit a **newly-gaining** page (Pipeline Analyzer, legacy Home Dashboard) — Ask Olly button now visible in header.
   - Click the button on a migrated page → drawer opens (or shows consent overlay), same as before.
