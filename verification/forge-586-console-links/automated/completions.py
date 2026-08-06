"""
Automated (deterministic, no-LLM) replay of the console-link verification for `cx completions`.

Source of truth: OLD_DIR/results/completions.jsonl (there is no dedicated run_completions.py
script - the old run drove this group directly; the jsonl is ground truth for exact args).

`completions` is entirely local (no Coralogix API calls at all), so there is nothing here
that touches real account data.

Covers:
  - `generate bash|zsh|fish|powershell` - pure stdout, no side effects, PASS last time.
    `zsh` was additionally exercised in json/agents format (to confirm -o does not affect
    the raw completion script text) - replayed the same way here.
  - `install bash --path <scratch>` - writes a completion file to an explicit --path
    pointed at OUR OWN scratch directory (never the user's real shell config), exactly as
    the old run did with its own scratch dir. Cleaned up after every run.

NOT covered here (see ../manual/completions.md):
  - `generate elvish` - FAILed last time with "Shell 'elvish' is not supported by cx
    completions". Per policy every FAIL goes to manual review rather than being asserted
    as an expected failure automatically, even though this one looks stable/deterministic.
  - `refresh` - has no --path/scratch override; it regenerates ALL previously-installed
    completions tracked in the real `~/.cx/config.toml` `managed_completions`, which on a
    real machine includes the user's actual shell completion file (e.g. ~/.zfunc/_cx).
    The old run explicitly skipped this to avoid overwriting a real file, and this script
    does the same.
"""

import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record

GROUP = "completions"
HERE = os.path.dirname(os.path.abspath(__file__))
BASE_DIR = os.path.dirname(HERE)
SCRATCH_DIR = os.path.join(BASE_DIR, "scratch_completions")


def run():
    for shell in ["bash", "zsh", "fish", "powershell"]:
        res = run_cx(["completions", "generate", shell], output_format="text")
        record(GROUP, f"generate {shell}", "text", res,
               notes="prints completion script to stdout, no side effects")

    for fmt in ["json", "agents"]:
        res = run_cx(["completions", "generate", "zsh"], output_format=fmt)
        record(GROUP, "generate zsh", fmt, res,
               notes="output format flag should not affect raw completion script text")

    os.makedirs(SCRATCH_DIR, exist_ok=True)
    install_path = os.path.join(SCRATCH_DIR, "_cx_bash_test")
    try:
        res = run_cx(["completions", "install", "bash", "--path", install_path], output_format="text")
        record(GROUP, "install bash --path <scratch>", "n/a", res,
               notes="uses bash (not any real registered shell) with --path pointed at our own scratch dir")
    finally:
        if os.path.exists(SCRATCH_DIR):
            shutil.rmtree(SCRATCH_DIR, ignore_errors=True)

    print(f"[{GROUP}] done")


if __name__ == "__main__":
    run()
