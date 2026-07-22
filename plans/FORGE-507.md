# FORGE-507 — Move "Attach file" into the "+" attachment menu

UI-only relocation. The standalone attach-file icon button (left of the model selector) moves into the `cx-olly-data-source-menu` "+" dropdown, alongside Skills / GitHub, following the existing icon+label `cxuiMenuItem` pattern. The icon-only tooltip is removed since the menu renders a text label.

## Design decisions
- **Trigger wiring:** file input + all attach logic (`onAttachFiles`, `addFiles`, `attachLimitReached`) stay owned by the parent `OllyChatPromptInputComponent`. Add a new `output` (trigger) + `input` (disabled state) to `DataSourceMenuComponent` and inline a `<button cxuiMenuItem>` in its template. Chosen over a standalone menu-item component because the attach state lives in the parent (unlike Skills/GitHub items which inject their own services).
- **Translation:** reuse existing `OLLY.CHAT.FILES.ATTACH` ("Attach file", `libs/i18n/cx/olly/en.json:128`). No new key added.
- **Icon:** `actions/attach.svg`, `size="sm"` to match sibling menu-item icons (Skills/GitHub use `size="sm"`).
- **Placement:** first item in the menu (above Skills), as the primary attach action.
- **Max-reached behavior (OPEN — defaulting):** ticket says remove the tooltip. So the menu item is `[disabled]` when `attachLimitReached()` but shows **no tooltip popup**. The old `MAX_REACHED` message is dropped from this control. Adjustable if the reporter wants the max-reached message surfaced elsewhere.

## Changes (in order)

### 1. `libs/olly/.../data-source-menu/data-source-menu.component.ts`
- Add `readonly attachFilesRequested = output<void>();`
- Add `readonly attachDisabled = input(false);`
- Add `CxuiMenuItem` to the `imports` array (import from `@cx/ui-menu`; `CxuiIcon`, `CxTranslatePipe` already imported).

### 2. `libs/olly/.../data-source-menu/data-source-menu.component.html`
Inside the `<cxui-menu>` (currently lines 42-47), add as the first child:
```html
<button
  cxuiMenuItem
  [disabled]="attachDisabled()"
  (click)="attachFilesRequested.emit()"
>
  <cxui-icon icon="actions/attach.svg" size="sm" />
  {{ 'OLLY.CHAT.FILES.ATTACH' | cxTranslate }}
</button>
```
(placed before `<cx-olly-skills-menu-item ... />`). No tooltip.

### 3. `libs/olly/.../prompt-input/chat-prompt-input.component.html`
- Line 196: wire the menu component:
  ```html
  <cx-olly-data-source-menu
    (skillSelected)="onSkillSelected($event)"
    (attachFilesRequested)="onAttachFiles()"
    [attachDisabled]="attachLimitReached()"
  />
  ```
- Remove the standalone attach button block (lines 200-210 — the `<button cxuiButton cxuiIconButton ... (click)="onAttachFiles()">` with `attachTooltip()`). The hidden `<input #fileInput type="file">` (lines 2-9) stays.

### 4. `libs/olly/.../prompt-input/chat-prompt-input.component.ts`
- Remove the now-unused `attachTooltip` computed (lines 207-217). It was the only consumer of `OLLY.CHAT.FILES.MAX_REACHED` in this file — verify no other reference before removing (grep `attachTooltip`).
- Keep `attachLimitReached` (now consumed via the menu input), `onAttachFiles`, `fileInput`, `onFilesSelected`, `addFiles`, and all upload logic unchanged.
- Do NOT remove `CxuiTooltipTrigger` from imports — it's still used by other toolbar buttons (stop/send, file error chips). Confirm by grep before touching imports.

## Edge cases / risks
- **Menu closes on click:** `cxuiMenuItem` click closes the dropdown, then `onAttachFiles()` opens the native file picker — expected/fine. Verify the file dialog still opens (the `fileInput` viewChild lookup must resolve; it lives on the parent and is unaffected).
- **Disabled state:** when `attachLimitReached()` is true (also true while upload-options load and `maxFilesPerMessage()` is 0), the item is disabled — matches prior button-disabled behavior.
- **Paste-to-attach** (`onPaste`) is independent of this button and unaffected.
- **No visual regression** to the remaining toolbar row (model selector / effort / send-stop) once the icon is removed — the `<div class="tw-flex tw-shrink-0 tw-items-center">` group now starts at the model button.
- Icon `actions/attach.svg` already exists (used by the current button).

## Verification
Run/check commands (from `frontend/`):
- Lint: `pnpm nx lint olly`
- Unit tests: `pnpm nx test olly` (no attach-specific unit tests today; `model-selection.spec.ts` unaffected)
- Build sanity: `pnpm nx build web-app`

Behavioral (requires running Olly chat against a backend — not runnable in this worktree without staging creds; do this in the impl/verify step):
- **Before:** attach-file icon sits left of the model-selection button with an "Attach file" tooltip; "+" menu shows only Skills / GitHub.
- **After:** no standalone attach icon in that row; "+" menu shows "Attach file" (icon+label) as first item; clicking it opens the native file picker and attaching a file behaves identically (chips render, dedupe, limits enforced); no tooltip on the menu item; when file limit reached the menu item is disabled.
- Capture a before/after screenshot (or short video) of the toolbar + open menu into `.saga/artifacts/`.

## Notes for reviewer
- Consider a `*.vitest.ts` component test for `DataSourceMenuComponent` asserting the attach item renders, emits `attachFilesRequested` on click, and is disabled when `attachDisabled` is true — optional given no existing test harness for this component, but low-cost and aligned with repo TDD guidance.
