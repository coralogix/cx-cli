## FORGE-351 — Suppress `mailto:` links in `@cx/ui-markdown`

### Summary of the fix

Bare email addresses in Olly assistant markdown (table artifact cells, paragraphs, list items) render today as clickable blue `mailto:` anchors because `CxuiMarkdown` instantiates `createIncremarkParser({ gfm: true })` in `libs/cx-ui/markdown/src/lib/markdown.ts:140,155,163,171`. The GFM autolink-literal extension converts any bare email into an mdast `Link` node with `url: 'mailto:<addr>'`, and `CxuiMarkdownNodes` (`markdown-nodes.ts:73`) renders every `Link` node via `CxuiMdLink` (`markdown-link.ts:20-31`) — a `target="_blank"` anchor. Table cells go through the same path via `MdTableCellRenderer` → `CxuiMarkdownNodes` (`markdown-table-cell-renderer.ts:18-28`), so the bug reproduces inside tables exactly as reported.

Fix: at the render layer inside `CxuiMarkdownNodes`, treat a `link` mdast node whose `url` starts with `mailto:` (case-insensitive) as plain phrasing content — render its children through a recursive `<cxui-markdown-nodes>` call instead of `<cxui-md-link>`, so no `<a>` element is produced.

The change lives in one file (`markdown-nodes.ts`). All consumers of `@cx/ui-markdown` (Olly chat, cases triage, settings/extensions tooltips) inherit the fix — this is correct because none of them have a legitimate reason to expose a clickable `mailto:` link, and the acceptance criteria explicitly require “no `Link` node with `href` starting with `mailto:`” from the pipeline.

### Approach comparison and choice

Two candidates surfaced in the ticket:

1. **Reconfigure the parser** — replace `{ gfm: true }` with an explicit micromark extension bundle (tables + task lists + strikethrough) omitting `gfmAutolinkLiteral`, at all 4 `createIncremarkParser` call sites.
2. **Post-process at render time** — in `CxuiMarkdownNodes`, treat `link` nodes whose URL is `mailto:` as plain text.

**Chosen: option 2 (render-level).** Reasons:

- One file changed, one line of predicate, one new template branch. No new dependencies, no risk of breaking streaming/table behavior.
- The success criteria say “no `<a href="mailto:...">` anchor” — this holds for BOTH GFM autolink literals AND explicit `[label](mailto:foo)` markdown links. The parser-level fix only handles autolinks, letting explicit `mailto:` links slip through. The render-level fix handles both uniformly.
- Case-ID auto-linking (`libs/olly/src/lib/shared/utils/linkify-cases.ts`) and real `http(s)://` URLs are untouched — the predicate is scoped strictly to `mailto:` prefix.
- Incremark’s `{ gfm: true }` internally wires the whole `micromark-extension-gfm` bundle; there is no per-feature toggle. Splitting it requires importing the individual sub-extensions (`micromark-extension-gfm-table`, `micromark-extension-gfm-strikethrough`, `micromark-extension-gfm-task-list-item`) plus their mdast counterparts, and wiring them via the `extensions` / `mdastExtensions` options — a much larger surface area to test.

### Files to change

1. **`libs/cx-ui/markdown/src/lib/markdown-nodes.ts`** (primary change)

   Inside the existing `@case ('link')` branch (around line 73), gate rendering:

   - If `node.url` starts with `mailto:` (case-insensitive), render the link’s children inline via a nested `<cxui-markdown-nodes [nodes]="asChildNodes(node)" />` — no anchor, no styling.
   - Otherwise, fall through to the existing `<cxui-md-link [node]="node" />` path.

   Implementation options (choose whichever fits the existing template style — either is acceptable):

   - **Template-side predicate** (preferred, most localized):
     ```
     @case ('link') {
       @if (isMailtoLink(node)) {
         <cxui-markdown-nodes [nodes]="asChildNodes(node)" />
       } @else {
         <cxui-md-link [node]="node" />
       }
     }
     ```
     and add a protected method:
     ```
     protected isMailtoLink(node: RootContent): boolean {
       return node.type === 'link' && node.url.toLowerCase().startsWith('mailto:');
     }
     ```
   - **Class-side computed guard** if the reviewer prefers keeping the switch shape flat: split into two `@case` guards via `@switch (isMailtoLink(node) ? 'mailtoLink' : node.type)` — less clean, avoid.

   Do NOT touch `CxuiMdLink`; it still renders every other link (http, https, custom protocols like `myapp://`, `cx-case:` from `linkifyCaseReferences`).

2. **`libs/cx-ui/markdown/src/lib/markdown.vitest.ts`** (test coverage)

   Add assertion tests inside the existing `describe('assertion', ...)` block (pattern matches the file’s existing `LinkClickMarkdown` fixtures — declare each test host `@Component` at module scope per the file’s comment on OXC compiler constraint):

   - `GIVEN markdown containing a bare email address / WHEN the markdown renders / THEN no anchor is rendered` — content `'Contact us at user@example.com for support.'`, assert `screen.getByRole('link')` returns no elements, and the plain email string is visible in the DOM.
   - `GIVEN a GFM table with an email cell / WHEN the markdown renders / THEN the cell contains no anchor` — content like ``| Name | Email |\n|------|-------|\n| Alice | alice@example.com |``. Poll for `.ag-row` count, then assert the cell containing `alice@example.com` renders no `<a>` element and the text `alice@example.com` is visible.
   - `GIVEN an explicit [label](mailto:foo) link / WHEN the markdown renders / THEN no anchor is rendered` — locks the “no mdast Link with mailto: href” contract from the acceptance criteria.
   - **Regression guards** — extend or duplicate the existing “renders links with correct attributes” test so it also asserts:
     - `[Click me](https://example.com)` still renders an `<a target="_blank" rel="noopener noreferrer">` (already covered by `InlineFormattingMarkdown` assertion at lines 540–549 — keep it green).
     - `[Case](cx-case:case-123)` still renders as a clickable anchor (add a small fixture — this is the concrete regression risk for the `linkifyCaseReferences` flow that the ticket calls out).

   Do NOT add new `visual` (screenshot) tests for this fix — plain-text emails don’t need a baseline. If a screenshot fixture happens to contain a mailto autolink and needs re-baselining, add the `update-snapshots` label on the PR per `.claude/rules/cx-ui.md`.

3. **(Optional) `libs/cx-ui/markdown/src/lib/markdown.stories.ts`**

   No story change required. The existing “Static” story already exercises links; add an email line only if you want the Storybook documentation to visibly demonstrate the new behavior. This is nice-to-have, not required.

### Files intentionally NOT changed

- `libs/cx-ui/markdown/src/lib/markdown-link.ts` — unchanged. Anchor rendering for all non-mailto URLs is preserved as-is.
- `libs/cx-ui/markdown/src/lib/markdown-table-cell-renderer.ts` — unchanged. It already routes to `CxuiMarkdownNodes`, which is where the fix lands.
- `libs/cx-ui/markdown/src/lib/markdown.ts` — unchanged. The four `createIncremarkParser({ gfm: true })` sites are left alone; tables, strikethrough, and task-list behavior stay intact.
- `libs/olly/src/lib/shared/utils/linkify-cases.ts` — unchanged. Case-ID auto-linking still runs upstream of the parser.
- `libs/olly/src/lib/components/table-artifact-view/table-artifact-view.component.ts` — out of scope (ag-grid path with no markdown parsing).

### Order of changes

1. Edit `markdown-nodes.ts` — add the `isMailtoLink` predicate and the template guard.
2. Add the assertion tests in `markdown.vitest.ts`.
3. Run the vitest-components target (see “Verification” below) and confirm all new + existing assertion tests pass.
4. Run lint + tsc-strict on `cx-ui-markdown`.
5. Save the “after” artifact (browser snapshot or screenshot) showing an email address rendered as plain text in a table cell, and add it to `.saga/artifacts/`.

### Edge cases and risks

- **Case sensitivity of the URL scheme.** GFM autolink literal always emits lowercase `mailto:`, but explicit markdown links can use `MAILTO:`. Predicate uses `toLowerCase().startsWith('mailto:')` to catch both.
- **URL-encoded / prefixed variants.** `mailto:` links with query params (`mailto:foo?subject=x`) still start with `mailto:` — predicate handles them. No further parsing needed.
- **Streaming.** The parser produces the same `Link` node incrementally. Because the check is on the mdast node during render, it works identically during streaming and after finalize.
- **`linkClick` output emissions.** The `linkClick` output is only fired from `CxuiMdLink.onClick`; if the anchor is never rendered, the output cannot fire for mailto content — that is intentional and desired (no navigation attempted).
- **Nested phrasing inside a `Link` node.** GFM autolink literal always emits a single `Text` child equal to the email address, so recursively rendering children via `<cxui-markdown-nodes>` produces the exact plain-text email in the DOM. Explicit `[label](mailto:foo)` may have styled children (bold, code) — recursing preserves that formatting minus the anchor, which is the correct fallback.
- **DomSanitizer’s `unsafe:` prefix on custom protocols.** Unrelated — the existing sanitizer handling in `CxuiMdLink.onClick` (lines 49–56) is only reached for links that still render; the mailto branch never hits it.
- **Consumer impact.** Cases triage, Olly execution rows, Olly assistant messages, and settings/extensions upgrade tooltips all route through `<cxui-markdown>`. None of them intentionally rely on rendering a clickable mailto link (verified by grep — no `mailto:` string in any consumer template or handler). Even the `handleOllyMarkdownLinkClick` handler in `linkify-cases.ts` only cares about artifact UUIDs and `cx-case:` hrefs. Safe globally.

### Verification

**Establishing the run commands** (from `CLAUDE.md` § “Testing”):

```
NX_TUI=false pnpm nx vitest-components cx-ui-markdown --run --browser.headless=true
pnpm nx lint cx-ui-markdown
pnpm nx run cx-ui-markdown:tsc-strict
```

**Environment note.** `node_modules` was not installed in the exploration worktree, so I could not execute the vitest suite as part of the plan step (`pnpm install` in this monorepo is heavy and out of scope for a plan). The implementation step should `pnpm install` before running the commands above.

**Behavior to observe before and after (the ticket reproduction):**

- Before: open the linked Olly chat (`https://c4c.app.eu2.coralogix.com/#/olly/chat/1636d74c-cef3-4720-a860-7cc5eb21fed8`) or any content that renders a GFM table with an email cell via `<cxui-markdown>`. The email is rendered as a blue underlined anchor. Inspecting the DOM shows `<a href="mailto:...">`.
- After: the same content shows plain text with no anchor, no underline, no click target. Inspecting the DOM shows a plain text span from `<cxui-markdown-nodes>`; no `<a>` element wraps the email.

**Regression targets to observe stay green:**

- `[Click me](https://example.com)` in the same message still renders as a clickable anchor. The existing `LinkClickMarkdown` vitest asserts this (`markdown.vitest.ts:743-754`).
- Case-ID references produced by `linkifyCaseReferences` (URLs of the form `cx-case:case-123`) still render as clickable anchors and still surface the `linkClick` event to the Olly click handler.

**Artifacts to capture** (per Saga workflow):

- `.saga/artifacts/before-mailto-table-cell.png` — screenshot of a table cell with an email currently rendered as a link (from the linked chat or a local Storybook fixture).
- `.saga/artifacts/after-mailto-table-cell.png` — screenshot of the same cell rendered as plain text after the change.

### Definition of done

- `<cxui-markdown>` never emits an `<a>` element whose `href` starts with `mailto:`, whether the email arrived via GFM autolink literal or explicit `[label](mailto:foo)` syntax.
- All new assertion tests in `markdown.vitest.ts` pass; the existing suite still passes (including the http-link, custom-protocol-link, and linkClick-emission tests).
- `pnpm nx lint cx-ui-markdown` and `pnpm nx run cx-ui-markdown:tsc-strict` succeed.
- No changes to `markdown-link.ts`, `markdown-table-cell-renderer.ts`, `markdown.ts`, `linkify-cases.ts`, or `table-artifact-view.component.ts`.
