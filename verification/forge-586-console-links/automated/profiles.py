"""
Deterministic replay of the `cx profiles` console-link verification
(PR #176).

Only `profiles list` is replayed here. It's the one read-only, side-effect-free
subcommand in this group: local command, no Coralogix API call, no console
link, and its 3 format-matrix entries in OLD_DIR/results/profiles.jsonl are
all PASS (with a note that `list` ignores `-o` and always renders the same
plain text table regardless of format -- that's expected/known, not a bug
to chase here).

`profiles add` (interactive-only, drives an `inquire`-based TUI wizard with
no non-interactive flags) and the `set-default`/`delete` steps that only
make sense against the throwaway profile `add` creates are deliberately
NOT replayed here -- see manual/profiles.md. There is no literal argv to
mechanically replay for `add` (the original run drove it via `expect`
sending raw keystrokes, which is not preserved anywhere in OLD_DIR), so
reconstructing and trusting a TUI-automation script without ever being able
to execute it first is a judgment call, not a mechanical replay.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from harness import run_cx, record  # noqa: E402

GROUP = "profiles"
FORMATS = ["text", "json", "agents"]


def run():
    for fmt in FORMATS:
        r = run_cx(["profiles", "list"], output_format=fmt)
        record(GROUP, "list", fmt, r)


if __name__ == "__main__":
    run()
