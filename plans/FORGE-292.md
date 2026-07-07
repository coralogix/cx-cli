## Summary

FORGE-292 lowers the Olly-chat code-block max height from 7.5 → 6.5 rows and brings the "result" (assistant-message) path to parity with the "step" path by routing its fenced code blocks through `OllyCodeBlockComponent`, which already provides the `OllyScrollFadeComponent` top/bottom fade overlays. The current "result" path uses a plain Tailwind `tw-max-h-[174px] tw-overflow-y-auto` class override on `CxuiMdCodeBlock` — that has a hard cut-off with no fade, and the fade cannot be implemented as a class-override because it needs the scroll-position signals inside `OllyScrollFadeComponent`.

## Current behavior (grounded from the worktree)

### Where the 7.5-line cap lives today
- `frontend/libs/olly/src/lib/components/code-block/olly-code-block.component.ts:49` — `readonly maxLines = input(7.5);` + `TOP_PADDING_PX (16) + maxLines * LINE_HEIGHT_PX (21) = 173.5 → round = 174px`. Wrapped in `<cx-olly-scroll-fade>` (fade overlays, `scroll-fade.component.html:9-20`) when `maxHeight` is set.
- `frontend/libs/olly/src/lib/components/chat/messages/assistant-message/chat-assistant-message.component.ts:63-64` — `codeBlock: 'tw-max-h-[174px] tw-overflow-y-auto [&_pre]:tw-whitespace-pre-wrap [&_pre]:[word-break:break-word] [&_pre.shiki]:tw-overflow-x-hidden'`, passed as `classOverrides.codeBlock` to `<cxui-markdown>` → `CxuiMdCodeBlock` (`frontend/libs/cx-ui/markdown/src/lib/markdown-code-block.ts:38-43`). **No fade.**

### Where `OllyCodeBlockComponent` (fade-capable) is used today
- `chat-execution-row-details.component.html:9-14` — tool-result details (step panel), passes `[unbounded]="true"` → **cap bypassed**, so the ticket's cap change won't affect this.
- `github-file-content-artifact-preview.component.html:1-5` — file-slice artifact preview, uses the **default** `maxLines=7.5` → affected by the default change.
- `rich-text.component.ts:21` — extracted `html` fenced blocks (via `splitRichTextSegments`) → uses default `maxLines=7.5` → affected.

`rich-text.component.ts` is the shared renderer for **both** step markdown (`OllyStepMarkdownComponent`) and result markdown (`OllyChatAssistantMessageComponent`). For non-HTML fenced code today, `splitRichTextSegments` keeps the fence in the `markdown` segment (`chat-execution.utils.ts:283-285`), so it renders through `CxuiMarkdown` → `CxuiMdCodeBlock` — the plain `<cxui-code-block>` path with **no fade**.

### 6.5-row height math
`TOP_PADDING_PX(16) + 6.5 * LINE_HEIGHT_PX(21) = 152.5 → round = 153px`. Matches the ticket's ~153px hint.

### Existing tests
- `libs/olly/src/lib/components/chat/messages/execution/chat-execution.utils.spec.ts:194-248` exercises `splitRichTextSegments` (including a `GIVEN a non-HTML fenced code block THEN it stays in the markdown stream` case that will need to change).
- No `.vitest.ts` for `OllyCodeBlockComponent`, `OllyScrollFadeComponent`, `OllyChatAssistantMessageComponent`, or `OllyChatRichTextComponent`.
- Existing markdown snapshot fixtures in `libs/cx-ui/markdown/src/lib/markdown.vitest.ts` are unaffected — the cx-ui/markdown lib is untouched.

## Approach — extend `splitRichTextSegments` to extract all fenced code blocks

Two options were considered:

1. **Extend `splitRichTextSegments` (chosen)** — treat every fenced code block the same way HTML fences are already treated today: pull it out of the markdown stream into its own segment, and render it via `OllyCodeBlockComponent`. **No changes to cx-ui/markdown.** Naturally gives step- and result-message code blocks the exact same rendering (fade + cap + line numbers + copy button).

2. **Wrap `CxuiMdCodeBlock` output with `cx-olly-scroll-fade`** — either forces olly to add a component-slot override into `cx-ui/markdown` (leaks olly-specific behavior into the design system), or requires post-render DOM manipulation. Rejected.

The chosen approach mirrors the existing HTML-fence extraction pattern already in `splitRichTextSegments`. It keeps olly's design-system boundary clean and satisfies success criterion 5 ("both step and result code blocks are visually and behaviorally consistent") in the strongest sense: both go through the exact same component.

### Side-effect callout (surface for reviewer)

Routing non-HTML fenced code through `OllyCodeBlockComponent` means result-message code blocks will also get:
- Line numbers (via the `cx-olly-code-block .shiki .line::before` CSS counter in `olly-code-block.component.css:34-48`).
- The always-visible, pinned top-right copy button (`olly-code-block.component.html:1-7`), replacing whatever hover-only button `CxuiCodeBlock` shows today.

Today, only HTML-fenced blocks in step/result paths and the file-slice artifact preview get line numbers. After this change, every fenced code block in Olly chat prose gets them. This is aligned with the ticket's "visually and behaviorally consistent" success criterion, but it is a broader UX change than a pure height tweak. **If the design intent is fade-only (no line numbers, no repositioned copy button) on result code blocks, flag this and we'll fall back to adding a `showLineNumbers`/`copyToClipboard` toggle on `OllyCodeBlockComponent` set to `false` for the extracted-markdown case.**

## Changes (in dependency order)

### 1. `frontend/libs/olly/src/lib/components/code-block/olly-code-block.component.ts`

Change the default cap:
```diff
- readonly maxLines = input(7.5);
+ readonly maxLines = input(6.5);
```
Height computation (`olly-code-block.component.ts:55-60`) is unchanged and already resolves to 153px with the new default.

### 2. `frontend/libs/olly/src/lib/components/chat/messages/execution/chat-execution.utils.ts`

Extend `splitRichTextSegments` (`chat-execution.utils.ts:239-312`) and the `RichTextSegment` type to emit a `code` segment for every fenced block (not just html/htm):

- New segment type:
  ```ts
  export type RichTextSegment =
    | { kind: 'markdown'; content: string }
    | { kind: 'html'; content: string }     // still emitted only for raw <html> documents (unchanged)
    | { kind: 'code'; content: string; language: string };
  ```
- In the fence-open branch (currently `if (fence) { ... }`):
  - If `isHtmlFence` (`html` / `htm`) → keep emitting `{ kind: 'html', content }` (preserves existing rendering + tests at lines 222-231 of the spec).
  - Otherwise → **new**: `flushMarkdown();` then push `{ kind: 'code', content: body.join('\n'), language }`. Do NOT re-emit the fence markers into the markdown stream.
- Streaming (unclosed fence): mirror the html behaviour — emit whatever body has arrived so far so partial code progressively renders. `body.join('\n')` already handles this.
- Non-fenced logic (raw `<html>` document detection, plain prose) is unchanged.

### 3. `frontend/libs/olly/src/lib/components/chat/messages/execution/chat-execution.utils.spec.ts`

Update the case at lines 233-239:
```ts
it(`GIVEN a non-HTML fenced code block
    THEN it becomes a code segment`, () => {
  const text = 'Run:\n```ts\nconst x = 1;\n```';
  expect(splitRichTextSegments(text)).toEqual([
    { kind: 'markdown', content: 'Run:' },
    { kind: 'code', content: 'const x = 1;', language: 'ts' },
  ]);
});
```
Add coverage for: (a) plain fenced code with no language → `language: ''` still routes via `code`; (b) streaming unclosed fence → single `code` segment with partial body; (c) prose + code + prose interleave.

### 4. `frontend/libs/olly/src/lib/components/chat/messages/execution/rich-text.component.ts`

Add a branch to the template `@if`/`@else` for the new `code` kind:
```ts
@if (segment.kind === 'html') {
  <cx-olly-code-block language="html" [code]="segment.content" />
} @else if (segment.kind === 'code') {
  <cx-olly-code-block [language]="segment.language || 'plaintext'" [code]="segment.content" />
} @else {
  <cxui-markdown ... />
}
```
(The two `cx-olly-code-block` branches can be collapsed into one that always uses `segment.language`, seeding `'html'` in the html-doc branch of `splitRichTextSegments`. Either way, keep the rendering identical.)

### 5. `frontend/libs/olly/src/lib/components/chat/messages/assistant-message/chat-assistant-message.component.ts`

Drop the now-obsolete `codeBlock` override — code blocks no longer flow through `CxuiMdCodeBlock` in this path:

```diff
protected readonly markdownClassOverrides = {
  h1: 'tw-text-lg',
  h2: 'tw-text-base',
  h3: 'tw-text-base',
  h4: 'tw-text-base',
  h5: 'tw-text-base',
  h6: 'tw-text-base',
- // Cap tall fenced code blocks (~7.5 lines) with an inner vertical scroll;
- // wrap long lines so there's no horizontal scroll.
- codeBlock:
-   'tw-max-h-[174px] tw-overflow-y-auto [&_pre]:tw-whitespace-pre-wrap [&_pre]:[word-break:break-word] [&_pre.shiki]:tw-overflow-x-hidden',
} satisfies CxuiMarkdownClassOverrides;
```

### 6. New component test — `frontend/libs/olly/src/lib/components/code-block/olly-code-block.component.vitest.ts`

- `libs/olly/vitest.config.mts` and `tests-browser-setup.ts` already exist and `libs/olly/**/*` is registered under the `vitest-components` plugin in `nx.json` — infrastructure is ready.
- Use isolated component tests (`@cx/testing-vitest/isolated`) since `OllyCodeBlockComponent` has no remote deps.
- Cover, per `single-action-test.md`:
  - `GIVEN default maxLines / WHEN 10-line code renders / THEN the container reports max-height 153px` (assert on the `[style.max-height]` of the `cx-olly-scroll-fade` inner container).
  - `GIVEN unbounded=true / THEN no scroll-fade wrapper is rendered` (regression guard against the maxHeight `@if`).
  - `GIVEN long code / WHEN not yet scrolled / THEN only the bottom fade overlay is visible` (assert overlay `span` in DOM after render).
  - `GIVEN long code / WHEN scrolled to bottom / THEN only the top fade overlay is visible` — drive scroll via `element.scrollTop = element.scrollHeight` then wait for `expect.poll`.
  - `GIVEN startLine=120 / THEN the gutter reserves 3 digits` (regression guard for FORGE-281's line-number logic at the new height, verifying `--olly-code-block-digits: 3`).

No component test is added for the assistant-message path — the utils spec above covers the routing correctness, and `OllyCodeBlockComponent`'s own vitest covers the fade behavior.

## Edge cases / risks

- **Streaming, partially-closed fences**: `splitRichTextSegments` already emits a segment for an unclosed HTML fence; the extended logic uses the same pattern for other languages. `OllyCodeBlockComponent` re-parses via Shiki on each content update — matches what already happens for streamed HTML fences.
- **`resolveDisplayedAssistantSource` "single text partition" path** (`chat-execution.utils.ts:1357-1379`) — the streamed display-event text is often newline-poor. Once a code segment is extracted, the surrounding prose remains in markdown segments; the split is line-based so it works on the newline-full `content` string as usual.
- **Nested fences inside blockquotes / list items** — current `splitRichTextSegments` doesn't handle indented fences (the regex requires `^\s*\`\`\``, then keeps consuming lines to the next `^\s*\`\`\``). Behaviour unchanged: nested fences continue to render through `CxuiMdCodeBlock` (no fade). Acceptable — matches today's HTML-fence handling and preserves markdown list/blockquote structure.
- **Cap change side-effects on `github-file-content-artifact-preview.component.html`** — the artifact preview also picks up the 6.5-line default. Success criterion 1 only mentions "step messages" but this preview is the primary consumer of the raw default. If the design wants to keep it at ≥7.5, pass an explicit `[maxLines]="7.5"` (or larger) in that template. Flag if the intent was to leave the artifact preview alone.
- **`tw-max-h-[174px]` removal** — after step 5, `CxuiMdCodeBlock` renders with no height cap (the class override is gone). This is fine because it's never reached — all code blocks are extracted by `splitRichTextSegments` before markdown parses them. If any code path bypassed `OllyChatRichTextComponent` and still passed content with fences straight to `CxuiMarkdown`, we'd regress. `grep` shows no such consumer inside `libs/olly`.
- **FORGE-281 (line numbers) regression guard** — success criterion 1 explicitly calls this out. The `lineNumberDigits` computation (`olly-code-block.component.ts:64-69`) is untouched; the new component test asserts it still works.

## Run / verify commands

From `frontend/CLAUDE.md`:

- **Lint**: `pnpm nx lint olly`
- **Unit tests** (updated `chat-execution.utils.spec.ts`): `pnpm nx test olly`
- **Component test** (new `olly-code-block.component.vitest.ts`): `NX_TUI=false pnpm nx vitest-components olly --run --browser.headless=true` (per `vitest-component-testing.md`)
- **Build sanity check**: `pnpm nx build web-app` (fast smoke)

### Manual visual verification

The interface for this change is the running Olly chat UI, and Olly requires live auth to a Coralogix staging/prod region — the change cannot be exercised end-to-end in the sandbox worktree. The implementation step should:

1. Run `pnpm run dstaging`, open the Olly chat.
2. Trigger a response whose "result" contains a long fenced code block (e.g. "give me a 20-line TypeScript example"). **Before**: hard cut-off at 174px, no fade. **After**: 153px cap, top/bottom gradient fades appear conditional on scroll position, half-row visible under the fade, line numbers + pinned copy button.
3. Trigger a step-message path (an HTML artifact preview or a `\`\`\`html` fenced block) and confirm it caps at 153px with the same fade.
4. Save both before/after screenshots (recording is preferable) into `.saga/artifacts/` — the orchestrator uploads to Linear/PR.

If staging auth is unavailable to the implementer, the vitest component test on `OllyCodeBlockComponent` plus the utils spec update are the automated evidence; visual verification then falls to the PR reviewer.

## Out of scope

Per the ticket: styling, syntax highlighting, copy-button behavior beyond what's required, line-numbering logic, and code blocks rendered outside Olly chat (other `cxui-code-block` consumers).