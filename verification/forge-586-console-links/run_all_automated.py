#!/usr/bin/env python3
"""
Run every deterministic, no-judgment-required check in automated/ and
regenerate the HTML report. Zero LLM tokens involved -- this is a plain
script.

    python3 run_all_automated.py             # run every group
    python3 run_all_automated.py iam alerts   # run only these groups

Requires a built `cx` binary (defaults to <repo_root>/target/debug/cx,
override with $CX_BIN) and a Coralogix profile with real access (defaults to
"kb-demo", override with $CX_TEST_PROFILE) -- see README.md. Every script
under automated/ creates only freshly, uniquely-named throwaway resources
and cleans them up itself; nothing here is safe to point at a team you don't
control.
"""

import importlib.util
import os
import subprocess
import sys
import time

BASE_DIR = os.path.dirname(os.path.abspath(__file__))
AUTOMATED_DIR = os.path.join(BASE_DIR, "automated")


def _discover_groups():
    names = []
    for fname in sorted(os.listdir(AUTOMATED_DIR)):
        if fname.endswith(".py") and not fname.startswith("_"):
            names.append(fname[: -len(".py")])
    return names


def _load_module(group):
    path = os.path.join(AUTOMATED_DIR, f"{group}.py")
    spec = importlib.util.spec_from_file_location(f"automated_{group.replace('-', '_')}", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    requested = sys.argv[1:]
    groups = requested if requested else _discover_groups()

    print(f"cx PR176 console-link verification -- running {len(groups)} group(s)")
    print(f"CX_BIN={os.environ.get('CX_BIN', '(default: <repo_root>/target/debug/cx)')}")
    print(f"CX_TEST_PROFILE={os.environ.get('CX_TEST_PROFILE', '(default: kb-demo)')}")
    print()

    results = {}
    for group in groups:
        path = os.path.join(AUTOMATED_DIR, f"{group}.py")
        if not os.path.exists(path):
            print(f"[{group}] SKIP -- no automated/{group}.py")
            results[group] = "missing"
            continue

        print(f"[{group}] running...")
        start = time.time()
        try:
            module = _load_module(group)
            module.run()
            results[group] = "ok"
            print(f"[{group}] done in {time.time() - start:.1f}s")
        except Exception as e:  # noqa: BLE001 -- one group's crash must not stop the rest
            results[group] = f"error: {e}"
            print(f"[{group}] ERRORED after {time.time() - start:.1f}s: {e}")
        print()

    print("=" * 60)
    print("Summary:")
    for group, status in results.items():
        print(f"  {group:20s} {status}")

    errored = [g for g, s in results.items() if s != "ok" and s != "missing"]

    print()
    print("Regenerating report.html from results/*.jsonl...")
    subprocess.run([sys.executable, os.path.join(BASE_DIR, "merge_report.py")], check=True)

    if errored:
        print()
        print(f"{len(errored)} group(s) errored -- see above. Exit 1.")
        sys.exit(1)


if __name__ == "__main__":
    main()
