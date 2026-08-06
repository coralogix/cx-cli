"""
Shared test harness for PR #176 ("View in Coralogix" console links) manual verification.

Every domain agent MUST use this module instead of hand-rolling subprocess calls or
report writing. record() is the durability guarantee: it appends to a per-group JSONL
file AND re-renders that group's HTML fragment after every single call, so a crash
mid-run never loses more than the one in-flight command.

Usage from a group script under automated/:

    import sys, os
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    from harness import run_cx, record

    res = run_cx(["alerts", "get", alert_id], output_format="json")
    record("alerts", "get", "json", res)

CX_BIN and PROFILE are overridable via env vars (CX_BIN, CX_TEST_PROFILE) since
this suite is tied to a specific real Coralogix team (kb-demo) that most
contributors won't have access to -- see README.md.
"""

import html
import json
import os
import subprocess
import time

BASE_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(os.path.dirname(BASE_DIR))
CX_BIN = os.environ.get("CX_BIN", os.path.join(REPO_ROOT, "target", "debug", "cx"))
PROFILE = os.environ.get("CX_TEST_PROFILE", "kb-demo")
RESULTS_DIR = os.path.join(BASE_DIR, "results")

os.makedirs(RESULTS_DIR, exist_ok=True)


def run_cx(args, output_format=None, extra_flags=None, timeout=90, profile=PROFILE):
    """Run the cx binary and return a result dict. Never raises on non-zero exit."""
    cmd = [CX_BIN, "-p", profile]
    if output_format:
        cmd += ["-o", output_format]
    if extra_flags:
        cmd += list(extra_flags)
    cmd += list(args)

    start = time.time()
    try:
        proc = subprocess.run(
            cmd, capture_output=True, text=True, timeout=timeout
        )
        exit_code = proc.returncode
        stdout = proc.stdout
        stderr = proc.stderr
    except subprocess.TimeoutExpired as e:
        exit_code = -1
        stdout = (e.stdout or b"").decode() if isinstance(e.stdout, bytes) else (e.stdout or "")
        stderr = f"TIMEOUT after {timeout}s"
    duration = time.time() - start

    return {
        "command": " ".join(cmd),
        "exit_code": exit_code,
        "stdout": stdout,
        "stderr": stderr,
        "duration_s": round(duration, 2),
    }


def record(group, subcommand, output_format, result, status=None, notes=""):
    """
    Append one test result to results/<group>.jsonl and re-render results/<group>.html.

    group: command group key, e.g. "alerts" (used as filename)
    subcommand: full subcommand path as tested, e.g. "get", "suppression-rules create",
                "create (setup)", "delete (cleanup)"
    output_format: "text" | "json" | "agents" | "n/a" (n/a for setup/cleanup calls that
                   aren't part of the output-format matrix)
    result: dict returned by run_cx()
    status: "PASS" | "FAIL" | "SKIPPED" | "CLI_BUG" | "BACKEND_BUG" | "HARD_TO_REPRODUCE"
            if None, inferred from exit_code (0 -> PASS, else FAIL).
            Use CLI_BUG when a follow-up check (or source read) confirms the defect is in
            cx's own request-building/response-handling code and is fixable in this repo
            (e.g. a required param the CLI never exposes as a flag). Use BACKEND_BUG when
            cx sends a well-formed request and the Coralogix backend is confirmed (or
            strongly indicated) to be the side that's wrong -- a silent no-op delete, a
            field-name mismatch on an undocumented endpoint, a route that 404s server-side.
            Both are distinct from a plain FAIL/exit-nonzero: a raw exit code alone can't
            tell you which side is at fault, or that a "successful" (exit 0) call didn't
            actually do what it claimed -- CLI_BUG/BACKEND_BUG exist for exactly that gap,
            and always deserve a `notes` explaining how it was confirmed (source line, or
            a follow-up list/get proving the mutation didn't take).
            Use HARD_TO_REPRODUCE when cx and the backend are BOTH behaving correctly --
            the call is legitimately rejected because a required state precondition isn't
            met -- but the specific real-world fixture this test targets (a fixed case/id
            with no create/list/reopen route of its own) has drifted into a terminal state
            with no way back through this CLI, so the exact same command can never pass
            again as scripted. Not a bug on either side; just note in `notes` what state
            the fixture is actually in and why it can't be reset.
    notes: free text, e.g. "sent real invite email to test-user@...", "no case existed to test against"
    """
    if status is None:
        status = "PASS" if result.get("exit_code") == 0 else "FAIL"

    entry = {
        "ts": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "group": group,
        "subcommand": subcommand,
        "output_format": output_format,
        "status": status,
        "notes": notes,
        **result,
    }

    jsonl_path = os.path.join(RESULTS_DIR, f"{group}.jsonl")
    with open(jsonl_path, "a") as f:
        f.write(json.dumps(entry) + "\n")
        f.flush()
        os.fsync(f.fileno())

    _render_group_html(group)
    return entry


def record_skip(group, subcommand, reason):
    """Log a subcommand that was intentionally not run (e.g. no create for `cases`)."""
    return record(
        group,
        subcommand,
        "n/a",
        {"command": "(not run)", "exit_code": None, "stdout": "", "stderr": "", "duration_s": 0},
        status="SKIPPED",
        notes=reason,
    )


def _load_group_entries(group):
    jsonl_path = os.path.join(RESULTS_DIR, f"{group}.jsonl")
    if not os.path.exists(jsonl_path):
        return []
    entries = []
    with open(jsonl_path) as f:
        for line in f:
            line = line.strip()
            if line:
                entries.append(json.loads(line))
    return entries


def _status_badge(status):
    color = {
        "PASS": "#1a7f37",
        "FAIL": "#cf222e",
        "SKIPPED": "#9a6700",
        "CLI_BUG": "#8250df",
        "BACKEND_BUG": "#9a6700",
        "HARD_TO_REPRODUCE": "#0969da",
    }.get(status, "#57606a")
    return f'<span style="display:inline-block;padding:2px 8px;border-radius:10px;font-size:12px;font-weight:600;color:#fff;background:{color}">{status}</span>'


def _render_group_html(group):
    entries = _load_group_entries(group)
    rows = []
    for e in entries:
        stdout_excerpt = e.get("stdout", "") or ""
        stderr_excerpt = e.get("stderr", "") or ""
        if len(stdout_excerpt) > 3000:
            stdout_excerpt = stdout_excerpt[:3000] + "\n... [truncated]"
        if len(stderr_excerpt) > 2000:
            stderr_excerpt = stderr_excerpt[:2000] + "\n... [truncated]"

        rows.append(f"""
        <tr>
          <td>{html.escape(e.get('subcommand',''))}</td>
          <td>{html.escape(str(e.get('output_format','')))}</td>
          <td>{_status_badge(e.get('status','?'))}</td>
          <td><code>{html.escape(str(e.get('exit_code')) if e.get('exit_code') is not None else '')}</code></td>
          <td><details><summary>show</summary><pre>{html.escape(e.get('command',''))}</pre></details></td>
          <td><details><summary>stdout</summary><pre>{html.escape(stdout_excerpt)}</pre></details></td>
          <td><details><summary>stderr</summary><pre>{html.escape(stderr_excerpt)}</pre></details></td>
          <td>{html.escape(e.get('notes',''))}</td>
        </tr>""")

    counts = {"PASS": 0, "FAIL": 0, "SKIPPED": 0}
    for e in entries:
        counts[e.get("status", "FAIL")] = counts.get(e.get("status", "FAIL"), 0) + 1

    out = f"""<!doctype html><html><head><meta charset="utf-8"><title>{html.escape(group)}</title>
<style>
body{{font-family:-apple-system,system-ui,sans-serif;margin:20px;color:#1f2328;background:#fff}}
table{{border-collapse:collapse;width:100%;font-size:13px}}
th,td{{border:1px solid #d0d7de;padding:6px 8px;text-align:left;vertical-align:top}}
th{{background:#f6f8fa;position:sticky;top:0}}
pre{{white-space:pre-wrap;word-break:break-all;max-width:600px;font-size:11px}}
code{{font-size:12px}}
</style></head><body>
<h2>{html.escape(group)}</h2>
<p>PASS: {counts.get('PASS',0)} &nbsp; FAIL: {counts.get('FAIL',0)} &nbsp; SKIPPED: {counts.get('SKIPPED',0)} &nbsp; CLI_BUG: {counts.get('CLI_BUG',0)} &nbsp; BACKEND_BUG: {counts.get('BACKEND_BUG',0)} &nbsp; HARD_TO_REPRODUCE: {counts.get('HARD_TO_REPRODUCE',0)} &nbsp; Total: {len(entries)}</p>
<table>
<tr><th>Subcommand</th><th>Format</th><th>Status</th><th>Exit</th><th>Command</th><th>Stdout</th><th>Stderr</th><th>Notes</th></tr>
{''.join(rows)}
</table>
</body></html>"""

    with open(os.path.join(RESULTS_DIR, f"{group}.html"), "w") as f:
        f.write(out)
