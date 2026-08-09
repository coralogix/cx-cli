"""
Render the console-link-eligible commands from results/*.jsonl as terminal-window
mockups, one card per (group, subcommand), with a tab per output format
(text/json/agents) that was actually captured for it. Filtered to exactly the
subcommands PR #176's own coverage table marks as expected to print a "View in
Coralogix" link (the same CANONICAL/EXCEPTIONS table merge_report.py uses) --
this is for eyeballing what those specific commands actually printed in each
format, not a general-purpose output browser.

stdout/stderr are concatenated as stderr-then-stdout to reconstruct what a real
terminal session looked like: cx writes progress/status lines to stderr via
eprintln! while a command runs, then its single final formatted result to stdout
at the very end -- so that order is a faithful replay, not an arbitrary merge.

Safe to re-run any time results/*.jsonl changes. If a (group, subcommand,
output_format) triple was recorded more than once (a script re-run), only the
most recent entry is shown.

Output: terminal_report.html (standalone) and terminal_report_content.html
(wrapper-free, for the Artifact tool) in this directory. Both gitignored --
generated, not source.
"""

import glob
import html
import json
import os
import re

from merge_report import GROUP_ORDER, badge, should_keep

BASE_DIR = os.path.dirname(os.path.abspath(__file__))
RESULTS_DIR = os.path.join(BASE_DIR, "results")
OUT_PATH = os.path.join(BASE_DIR, "terminal_report.html")
ARTIFACT_OUT_PATH = os.path.join(BASE_DIR, "terminal_report_content.html")

FORMAT_ORDER = ["text", "json", "agents", "n/a"]
FORMAT_LABEL = {"text": "text", "json": "json", "agents": "agents", "n/a": "setup/cleanup"}
MAX_BLOCK_CHARS = 4000


def load_all():
    """Return {(group, subcommand, output_format): latest_entry}, deduped by ts."""
    best = {}
    for path in sorted(glob.glob(os.path.join(RESULTS_DIR, "*.jsonl"))):
        group = os.path.basename(path)[: -len(".jsonl")]
        with open(path) as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                e = json.loads(line)
                key = (group, e.get("subcommand", ""), e.get("output_format", "n/a"))
                if key not in best or e["ts"] > best[key]["ts"]:
                    best[key] = e
    return best


def cmd_display(command):
    """Cosmetic only: swap the absolute binary path for the bare `cx` most users type."""
    parts = command.split()
    if not parts:
        return command
    return "cx " + " ".join(parts[1:])


def terminal_block(entry):
    stderr = (entry.get("stderr") or "").rstrip("\n")
    stdout = (entry.get("stdout") or "").rstrip("\n")
    merged = "\n".join(p for p in (stderr, stdout) if p)
    total_len = len(merged)
    if total_len > MAX_BLOCK_CHARS:
        merged = merged[:MAX_BLOCK_CHARS] + f"\n… [truncated, {total_len:,} chars total]"
    if not merged:
        merged = "(no output)"

    status = entry.get("status", "FAIL")
    exit_code = entry.get("exit_code")
    exit_str = str(exit_code) if exit_code is not None else "—"
    duration = entry.get("duration_s")
    duration_str = f"{duration:.2f}s" if isinstance(duration, (int, float)) else "—"

    return f"""
        <div class="term">
          <div class="term-bar">
            <span class="term-dots"><span></span><span></span><span></span></span>
            <span class="term-cmd mono">$ {html.escape(cmd_display(entry.get('command', '')))}</span>
            <span class="term-meta mono">{badge(status)}<span class="term-exit">exit {html.escape(exit_str)}</span><span class="term-dur">{html.escape(duration_str)}</span></span>
          </div>
          <pre class="term-body mono">{html.escape(merged)}</pre>
        </div>"""


def render_card(card_idx, group, subcommand, entries_by_format):
    present = [f for f in FORMAT_ORDER if f in entries_by_format]
    statuses = [entries_by_format[f].get("status", "FAIL") for f in present]
    dominant = "fail" if any(s in ("FAIL", "CLI_BUG") for s in statuses) else (
        "skip" if any(s in ("SKIPPED", "BACKEND_BUG", "HARD_TO_REPRODUCE") for s in statuses) else "pass"
    )

    if len(present) == 1:
        fmt = present[0]
        tabs_html = ""
        panels_html = f'<div class="panels">{terminal_block(entries_by_format[fmt])}</div>'
        single_tag = f'<span class="single-fmt mono">{FORMAT_LABEL[fmt]}</span>'
    else:
        name = f"fmt-{card_idx}"
        radios = []
        tabs = []
        panels = []
        for i, fmt in enumerate(present):
            rid = f"{name}-{fmt.replace('/', '_')}"
            checked = " checked" if i == 0 else ""
            radios.append(f'<input type="radio" name="{name}" id="{rid}" class="fmt-radio" data-fmt="{fmt}"{checked}>')
            tabs.append(f'<label for="{rid}" class="fmt-tab" data-fmt="{fmt}">{FORMAT_LABEL[fmt]}</label>')
            panels.append(f'<div class="panel panel-{fmt.replace("/", "_")}">{terminal_block(entries_by_format[fmt])}</div>')
        tabs_html = "".join(radios) + f'<div class="fmt-tabs">{"".join(tabs)}</div>'
        panels_html = f'<div class="panels" data-group="{name}">{"".join(panels)}</div>'
        single_tag = ""

    return f"""
        <article class="card {dominant}">
          <div class="card-head">
            <h3 class="mono">{html.escape(subcommand)}</h3>
            {single_tag}
          </div>
          {tabs_html}
          {panels_html}
        </article>"""


def main():
    best = load_all()

    by_group = {}
    for (group, subcommand, fmt), entry in best.items():
        if not should_keep(group, subcommand)[0]:
            continue
        by_group.setdefault(group, {}).setdefault(subcommand, {})[fmt] = entry

    ordered_groups = [g for g in GROUP_ORDER if g in by_group]
    ordered_groups += sorted(g for g in by_group if g not in GROUP_ORDER)

    total_counts = {"PASS": 0, "FAIL": 0, "SKIPPED": 0}
    card_count = 0
    invocation_count = sum(len(fmts) for subcmds in by_group.values() for fmts in subcmds.values())
    nav = []
    sections = []
    card_idx = 0

    for group in ordered_groups:
        subcmds = by_group[group]
        ordered_subcmds = sorted(
            subcmds.items(),
            key=lambda kv: min(e["ts"] for e in kv[1].values()),
        )

        group_counts = {"PASS": 0, "FAIL": 0, "SKIPPED": 0}
        cards_html = []
        for subcommand, entries_by_format in ordered_subcmds:
            for e in entries_by_format.values():
                s = e.get("status", "FAIL")
                group_counts[s] = group_counts.get(s, 0) + 1
                total_counts[s] = total_counts.get(s, 0) + 1
            cards_html.append(render_card(card_idx, group, subcommand, entries_by_format))
            card_idx += 1
            card_count += 1

        dominant = "fail" if group_counts.get("FAIL", 0) else ("skip" if group_counts.get("SKIPPED", 0) else "pass")
        anchor = re.sub(r"[^a-z0-9-]", "-", group.lower())
        nav.append(f"""<a class="chip {dominant}" href="#{anchor}">
            <span class="chip-label">{html.escape(group)}</span>
            <span class="chip-counts">{len(ordered_subcmds)} cmd{'s' if len(ordered_subcmds) != 1 else ''}</span>
        </a>""")

        sections.append(f"""
        <section class="group" id="{anchor}">
          <div class="group-head">
            <h2 class="mono">{html.escape(group)}</h2>
            <span class="group-count">{len(ordered_subcmds)} subcommand{'s' if len(ordered_subcmds) != 1 else ''} &middot; {sum(len(v) for v in subcmds.values())} invocations</span>
          </div>
          {''.join(cards_html)}
        </section>""")

    CSS = """:root {
  --bg: #f4f5f8;
  --surface: #ffffff;
  --surface-2: #ececf2;
  --border: #dadde6;
  --ink: #1a1f2b;
  --ink-dim: #666c7d;
  --accent: #9a6112;
  --accent-bg: #f3e6cf;
  --pass: #1a7f37;
  --pass-bg: #dcf5e2;
  --fail: #cf222e;
  --fail-bg: #fce4e4;
  --skip: #9a6700;
  --skip-bg: #faf0d6;
  --cli-bug: #8250df;
  --cli-bug-bg: #ede4fb;
  --hard-repro: #0969da;
  --hard-repro-bg: #ddf4ff;
  color-scheme: light dark;
}
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --bg: #0b0e14;
    --surface: #11151f;
    --surface-2: #171c29;
    --border: #262d40;
    --ink: #dde1ec;
    --ink-dim: #8a90a4;
    --accent: #e0a94a;
    --accent-bg: #2e2210;
    --pass: #4ce06b;
    --pass-bg: #123a1d;
    --fail: #ff6b64;
    --fail-bg: #3a1414;
    --skip: #e8b649;
    --skip-bg: #3a2c0c;
    --cli-bug: #c297ff;
    --cli-bug-bg: #2c1e42;
    --hard-repro: #79c0ff;
    --hard-repro-bg: #10243e;
  }
}
:root[data-theme="dark"] {
  --bg: #0b0e14; --surface: #11151f; --surface-2: #171c29; --border: #262d40;
  --ink: #dde1ec; --ink-dim: #8a90a4; --accent: #e0a94a; --accent-bg: #2e2210;
  --pass: #4ce06b; --pass-bg: #123a1d; --fail: #ff6b64; --fail-bg: #3a1414;
  --skip: #e8b649; --skip-bg: #3a2c0c; --cli-bug: #c297ff; --cli-bug-bg: #2c1e42;
  --hard-repro: #79c0ff; --hard-repro-bg: #10243e;
}
:root[data-theme="light"] {
  --bg: #f4f5f8; --surface: #ffffff; --surface-2: #ececf2; --border: #dadde6;
  --ink: #1a1f2b; --ink-dim: #666c7d; --accent: #9a6112; --accent-bg: #f3e6cf;
  --pass: #1a7f37; --pass-bg: #dcf5e2; --fail: #cf222e; --fail-bg: #fce4e4;
  --skip: #9a6700; --skip-bg: #faf0d6; --cli-bug: #8250df; --cli-bug-bg: #ede4fb;
  --hard-repro: #0969da; --hard-repro-bg: #ddf4ff;
}

/* Terminal windows deliberately stay one fixed dark theme regardless of page
   theme, same as a real terminal emulator does -- colors set explicitly, not
   from page tokens, so this renders identically on either ground. */
.term {
  --term-bg: #1b1e26;
  --term-bar: #23272f;
  --term-fg: #d7dbe4;
  --term-fg-dim: #7d8493;
  --term-accent: #d9a441;
}

* { box-sizing: border-box; }
html, body { overflow-x: hidden; }
body {
  margin: 0;
  background: var(--bg);
  color: var(--ink);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  font-size: 15px;
  line-height: 1.5;
}
.mono { font-family: ui-monospace, "SF Mono", "Cascadia Code", Menlo, Consolas, monospace; }

header {
  position: sticky;
  top: 0;
  z-index: 5;
  background: var(--bg);
  border-bottom: 1px solid var(--border);
  padding: 20px clamp(16px, 4vw, 40px) 14px;
}
.title-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 8px 24px;
}
h1 {
  font-family: ui-monospace, "SF Mono", "Cascadia Code", Menlo, Consolas, monospace;
  font-size: 20px;
  font-weight: 600;
  margin: 0;
  letter-spacing: -0.01em;
  text-wrap: balance;
}
h1 .dim-part { color: var(--ink-dim); font-weight: 500; }
.subtitle { margin: 4px 0 0; font-size: 12px; color: var(--ink-dim); max-width: 70ch; }
.stat-row {
  display: flex;
  gap: 18px;
  align-items: baseline;
  font-family: ui-monospace, "SF Mono", "Cascadia Code", Menlo, Consolas, monospace;
}
.stat { display: flex; flex-direction: column; align-items: flex-end; }
.stat .n { font-size: 22px; font-weight: 700; font-variant-numeric: tabular-nums; }
.stat .l { font-size: 10px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--ink-dim); }
.stat.pass .n { color: var(--pass); }
.stat.fail .n { color: var(--fail); }
.stat.skip .n { color: var(--skip); }

.chiprow {
  margin-top: 14px;
  display: flex;
  gap: 8px;
  overflow-x: auto;
  padding-bottom: 4px;
}
.chip {
  flex: 0 0 auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 6px 10px;
  border-radius: 3px;
  border: 1px solid var(--border);
  border-left: 3px solid var(--ink-dim);
  background: var(--surface);
  text-decoration: none;
  color: var(--ink);
  font-family: ui-monospace, "SF Mono", "Cascadia Code", Menlo, Consolas, monospace;
  font-size: 11px;
  white-space: nowrap;
}
.chip.pass { border-left-color: var(--pass); }
.chip.fail { border-left-color: var(--fail); }
.chip.skip { border-left-color: var(--skip); }
.chip-label { font-weight: 600; }
.chip-counts { color: var(--ink-dim); font-variant-numeric: tabular-nums; }

main { padding: 24px clamp(16px, 4vw, 40px) 80px; max-width: 1200px; margin: 0 auto; }

.group { margin-bottom: 36px; }
.group-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 4px 16px;
  padding-bottom: 8px;
  margin-bottom: 14px;
  border-bottom: 1px solid var(--border);
  scroll-margin-top: 160px;
}
.group-head h2 { font-size: 15px; font-weight: 700; margin: 0; }
.group-count { font-size: 11px; color: var(--ink-dim); }

.card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-left: 3px solid var(--ink-dim);
  border-radius: 4px;
  padding: 12px 14px;
  margin-bottom: 14px;
}
.card.pass { border-left-color: var(--pass); }
.card.fail { border-left-color: var(--fail); }
.card.skip { border-left-color: var(--skip); }
.card-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 8px;
}
.card-head h3 { font-size: 13px; font-weight: 650; margin: 0; }
.single-fmt {
  font-size: 10.5px;
  color: var(--ink-dim);
  text-transform: uppercase;
  letter-spacing: 0.06em;
}

.fmt-radio { position: absolute; opacity: 0; pointer-events: none; }
.fmt-tabs { display: flex; gap: 2px; margin-bottom: 8px; }
.fmt-tab {
  cursor: pointer;
  padding: 3px 10px;
  font-size: 11px;
  font-family: ui-monospace, "SF Mono", "Cascadia Code", Menlo, Consolas, monospace;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--ink-dim);
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: 3px;
  user-select: none;
}
.fmt-radio:checked + .fmt-tab {
  background: var(--surface);
  color: var(--accent);
  border-color: var(--accent);
}
.fmt-radio:focus-visible + .fmt-tab { outline: 2px solid var(--accent); outline-offset: 1px; }

.panel { display: none; }
input.fmt-radio[data-fmt="text"]:checked ~ .panels .panel-text,
input.fmt-radio[data-fmt="json"]:checked ~ .panels .panel-json,
input.fmt-radio[data-fmt="agents"]:checked ~ .panels .panel-agents { display: block; }

.term {
  border-radius: 6px;
  overflow: hidden;
  border: 1px solid var(--border);
}
.term-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 10px;
  background: var(--term-bar);
}
.term-dots { display: flex; gap: 5px; flex: 0 0 auto; }
.term-dots span {
  width: 9px; height: 9px; border-radius: 50%;
  background: #ffffff22;
}
.term-cmd {
  flex: 1 1 auto;
  min-width: 0;
  overflow-x: auto;
  white-space: nowrap;
  font-size: 11.5px;
  color: var(--term-accent);
  scrollbar-width: thin;
}
.term-meta {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 10.5px;
  color: var(--term-fg-dim);
}
.term-exit, .term-dur { font-variant-numeric: tabular-nums; }
.term-body {
  margin: 0;
  padding: 10px 12px;
  background: var(--term-bg);
  color: var(--term-fg);
  font-size: 11.5px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 420px;
  overflow-y: auto;
}

.badge {
  display: inline-block;
  padding: 1px 7px;
  border-radius: 2px;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.02em;
}
.badge.pass { color: var(--pass); background: var(--pass-bg); }
.badge.fail { color: var(--fail); background: var(--fail-bg); }
.badge.skip { color: var(--skip); background: var(--skip-bg); }
.badge.cli-bug { color: var(--cli-bug); background: var(--cli-bug-bg); }
.badge.backend-bug { color: var(--skip); background: var(--skip-bg); }
.badge.hard-to-reproduce { color: var(--hard-repro); background: var(--hard-repro-bg); }

a { color: var(--accent); }
::selection { background: var(--accent-bg); }
"""

    grand_total = sum(total_counts.values())
    pass_pct = round(100 * total_counts.get("PASS", 0) / grand_total) if grand_total else 0

    page_body = f"""<style>
{CSS}
</style>
<header>
  <div class="title-row">
    <div>
      <h1>cx <span class="dim-part">&mdash; console-link commands, terminal output (PR #176)</span></h1>
      <p class="subtitle">Only the subcommands PR #176's coverage table marks as expected to print a "View in Coralogix" link, replayed as they appeared in a real terminal (stderr progress lines, then the final stdout result). Tabs switch between the output formats actually captured for that command.</p>
    </div>
    <div class="stat-row">
      <div class="stat pass"><span class="n">{total_counts.get('PASS',0)}</span><span class="l">pass</span></div>
      <div class="stat fail"><span class="n">{total_counts.get('FAIL',0)}</span><span class="l">fail</span></div>
      <div class="stat skip"><span class="n">{total_counts.get('SKIPPED',0)}</span><span class="l">skip</span></div>
      <div class="stat"><span class="n">{card_count}</span><span class="l">commands &middot; {invocation_count} calls &middot; {pass_pct}%</span></div>
    </div>
  </div>
  <nav class="chiprow">{''.join(nav)}</nav>
</header>
<main>
  {''.join(sections)}
</main>
"""

    full_doc = f"""<!doctype html><html><head><meta charset="utf-8">
<title>cx CLI &mdash; console-link commands, terminal output (PR #176)</title>
</head><body>
{page_body}
</body></html>"""

    with open(OUT_PATH, "w") as f:
        f.write(full_doc)
    with open(ARTIFACT_OUT_PATH, "w") as f:
        f.write(page_body)
    print(
        f"Wrote {OUT_PATH} and {ARTIFACT_OUT_PATH} "
        f"({card_count} commands, {invocation_count} invocations across {len(ordered_groups)} groups)"
    )


if __name__ == "__main__":
    main()
