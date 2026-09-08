#!/usr/bin/env python3
"""Snapshot cx CLI distribution proxies: GitHub Release binaries + crates.io.

GitHub only publishes a running total per file. This script records that
total once per day and writes daily.csv so later rows include a per-day
delta. crates.io already has real daily counts.

Requires: python3. GitHub: GITHUB_TOKEN, or `gh` logged in.

Usage:
  python3 scripts/cli-download-stats.py
  python3 scripts/cli-download-stats.py --json
  python3 scripts/cli-download-stats.py --stats-dir ./stats-data
  python3 scripts/cli-download-stats.py --self-test
"""

from __future__ import annotations

import argparse
import csv
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request
from collections import defaultdict
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from typing import Any

GITHUB_REPO = "coralogix/cx-cli"
CRATE = "coralogix-cli"
UA = "cx-cli-download-stats"
CSV_FIELDS = [
    "date",
    "github_latest_tag",
    "github_latest_cumulative",
    "github_latest_daily",
    "github_all_binary_cumulative",
    "github_all_binary_daily",
    "crates_downloads",
]
README = """# cx CLI download snapshots

Updated daily by `.github/workflows/snapshot-download-stats.yml`.

- `daily.csv` — one row per UTC day. GitHub `*_daily` is empty on the first
  snapshot and on a day the latest release tag changes (not comparable).
- `snapshots.jsonl` — full payload for debugging.

GitHub binaries mix Homebrew, the curl installer, and zip downloads.
crates.io is `cargo install` only. Do not add the two series together.
"""


def is_binary(name: str) -> bool:
    n = name.lower()
    if any(s in n for s in ("checksum", "sbom", "cyclonedx", ".sig", ".pem")):
        return False
    return n.endswith(".tar.gz") or n.endswith(".zip")


def platform(name: str) -> str:
    if "aarch64-apple-darwin" in name:
        return "macos-arm64"
    if "x86_64-apple-darwin" in name:
        return "macos-x86_64"
    if "aarch64-unknown-linux" in name:
        return "linux-arm64"
    if "x86_64-unknown-linux" in name:
        return "linux-x86_64"
    if "windows" in name:
        return "windows-x86_64"
    return "other"


def http_json(url: str, headers: dict[str, str] | None = None) -> Any:
    hdrs = {"User-Agent": UA, "Accept": "application/json"}
    if headers:
        hdrs.update(headers)
    req = urllib.request.Request(url, headers=hdrs)
    with urllib.request.urlopen(req) as resp:
        return json.load(resp), dict(resp.headers)


def github_auth_headers() -> dict[str, str]:
    headers = {
        "User-Agent": UA,
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
    }
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return headers


def gh_releases() -> list:
    headers = github_auth_headers()
    url = f"https://api.github.com/repos/{GITHUB_REPO}/releases?per_page=100"
    releases: list = []
    while url:
        try:
            body, resp_headers = http_json(url, headers)
        except urllib.error.HTTPError:
            if os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN"):
                raise
            raw = subprocess.check_output(
                ["gh", "api", "--paginate", f"repos/{GITHUB_REPO}/releases?per_page=100"],
                text=True,
            )
            return json.loads(raw)
        releases.extend(body)
        url = next_link(resp_headers.get("Link") or resp_headers.get("link"))
    return releases


def next_link(link_header: str | None) -> str | None:
    if not link_header:
        return None
    for part in link_header.split(","):
        section = part.strip()
        if 'rel="next"' in section:
            start = section.find("<") + 1
            end = section.find(">")
            return section[start:end]
    return None


def crates_summary() -> dict:
    body, _ = http_json(f"https://crates.io/api/v1/crates/{CRATE}")
    return body["crate"]


def crates_downloads() -> dict:
    body, _ = http_json(f"https://crates.io/api/v1/crates/{CRATE}/downloads")
    return body


def crate_daily(payload: dict) -> dict[str, int]:
    by_date: dict[str, int] = defaultdict(int)
    for row in payload.get("version_downloads") or []:
        by_date[row["date"]] += int(row["downloads"])
    for row in (payload.get("meta") or {}).get("extra_downloads") or []:
        by_date[row["date"]] += int(row["downloads"])
    return dict(by_date)


def last_n_days(by_date: dict[str, int], n: int, today: str) -> int:
    end = date.fromisoformat(today)
    start = end - timedelta(days=n - 1)
    return sum(v for d, v in by_date.items() if start.isoformat() <= d <= today)


def collect() -> dict:
    now = datetime.now(timezone.utc)
    today = now.date().isoformat()
    releases = gh_releases()
    crate = crates_summary()
    daily = crate_daily(crates_downloads())

    gh_versions = []
    platforms: dict[str, int] = defaultdict(int)
    binary_total = 0
    other_total = 0
    latest_binary = 0
    latest_tag = None

    for rel in releases:
        tag = rel.get("tag_name") or ""
        pub = (rel.get("published_at") or "")[:10]
        binary = 0
        other = 0
        for asset in rel.get("assets") or []:
            name = asset["name"]
            count = int(asset.get("download_count") or 0)
            if is_binary(name):
                binary += count
                platforms[platform(name)] += count
            else:
                other += count
        binary_total += binary
        other_total += other
        gh_versions.append(
            {"tag": tag, "published": pub, "binary_downloads": binary, "other_assets": other}
        )
        if latest_tag is None and rel.get("draft") is not True:
            latest_tag = tag
            latest_binary = binary

    return {
        "snapshot_at": now.replace(microsecond=0).isoformat(),
        "date": today,
        "github": {
            "latest_tag": latest_tag,
            "latest_binary_downloads": latest_binary,
            "all_releases_binary_downloads": binary_total,
            "other_asset_downloads": other_total,
            "by_platform": dict(sorted(platforms.items(), key=lambda kv: -kv[1])),
            "by_version": gh_versions,
        },
        "crates_io": {
            "crate": CRATE,
            "newest_version": crate.get("newest_version"),
            "downloads_all_time": crate.get("downloads"),
            "downloads_recent": crate.get("recent_downloads"),
            "last_7_days": last_n_days(daily, 7, today),
            "last_30_days": last_n_days(daily, 30, today),
            "daily": dict(sorted(daily.items())),
        },
    }


def load_snapshots(path: Path) -> list[dict]:
    if not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def previous_snapshot(snapshots: list[dict], today: str) -> dict | None:
    earlier = [s for s in snapshots if snapshot_date(s) < today]
    return earlier[-1] if earlier else None


def snapshot_date(snapshot: dict) -> str:
    if snapshot.get("date"):
        return snapshot["date"][:10]
    return str(snapshot.get("snapshot_at") or "")[:10]


def github_latest_daily(prev: dict | None, curr: dict) -> str:
    if prev is None:
        return ""
    if prev["github"]["latest_tag"] != curr["github"]["latest_tag"]:
        return ""
    delta = curr["github"]["latest_binary_downloads"] - prev["github"]["latest_binary_downloads"]
    return str(max(0, delta))


def github_all_daily(prev: dict | None, curr: dict) -> str:
    if prev is None:
        return ""
    delta = (
        curr["github"]["all_releases_binary_downloads"]
        - prev["github"]["all_releases_binary_downloads"]
    )
    return str(max(0, delta))


def daily_row(prev: dict | None, curr: dict) -> dict[str, str]:
    day = snapshot_date(curr)
    crates = curr["crates_io"].get("daily") or {}
    crates_today = crates.get(day, "")
    if crates_today == "":
        crates_val = ""
    else:
        crates_val = str(crates_today)
    return {
        "date": day,
        "github_latest_tag": str(curr["github"]["latest_tag"] or ""),
        "github_latest_cumulative": str(curr["github"]["latest_binary_downloads"]),
        "github_latest_daily": github_latest_daily(prev, curr),
        "github_all_binary_cumulative": str(curr["github"]["all_releases_binary_downloads"]),
        "github_all_binary_daily": github_all_daily(prev, curr),
        "crates_downloads": crates_val,
    }


def upsert_csv(path: Path, row: dict[str, str]) -> None:
    by_date: dict[str, dict[str, str]] = {}
    if path.exists():
        with path.open(encoding="utf-8", newline="") as f:
            for existing in csv.DictReader(f):
                if existing.get("date"):
                    by_date[existing["date"]] = existing
    by_date[row["date"]] = row
    with path.open("w", encoding="utf-8", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=CSV_FIELDS)
        writer.writeheader()
        for day in sorted(by_date):
            writer.writerow({k: by_date[day].get(k, "") for k in CSV_FIELDS})


def write_snapshots(path: Path, snapshots: list[dict], curr: dict) -> None:
    today = snapshot_date(curr)
    kept = [s for s in snapshots if snapshot_date(s) != today]
    kept.append(curr)
    path.write_text(
        "".join(json.dumps(s, separators=(",", ":")) + "\n" for s in kept),
        encoding="utf-8",
    )


def write_stats_dir(directory: Path, curr: dict) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    jsonl = directory / "snapshots.jsonl"
    csv_path = directory / "daily.csv"
    snapshots = load_snapshots(jsonl)
    prev = previous_snapshot(snapshots, snapshot_date(curr))
    row = daily_row(prev, curr)
    write_snapshots(jsonl, snapshots, curr)
    upsert_csv(csv_path, row)
    (directory / "README.md").write_text(README, encoding="utf-8")


def print_text(data: dict, prev: dict | None = None) -> None:
    gh = data["github"]
    cr = data["crates_io"]
    row = daily_row(prev, data)
    print(f"snapshot  {data['snapshot_at']}")
    print()
    print("GitHub Releases (brew + curl + zip)")
    print(f"  latest {gh['latest_tag']}:  {gh['latest_binary_downloads']} cumulative")
    if row["github_latest_daily"]:
        print(f"  downloads since last snapshot: {row['github_latest_daily']}")
    else:
        print("  daily: (first snapshot or new tag — no daily number yet)")
    print(f"  all versions cumulative: {gh['all_releases_binary_downloads']}")
    print("  by platform:")
    for plat, n in gh["by_platform"].items():
        print(f"    {plat:16} {n}")
    print()
    print("crates.io (cargo install)")
    print(
        f"  newest {cr['newest_version']}: all-time {cr['downloads_all_time']}, recent {cr['downloads_recent']}"
    )
    print(f"  last 7 days:  {cr['last_7_days']}")
    print(f"  last 30 days: {cr['last_30_days']}")
    print(f"  today (UTC, if crates.io has it): {row['crates_downloads'] or 'not reported yet'}")


def self_test() -> None:
    prev = {
        "date": "2026-09-07",
        "github": {
            "latest_tag": "v0.1.24",
            "latest_binary_downloads": 1900,
            "all_releases_binary_downloads": 29900,
        },
        "crates_io": {"daily": {"2026-09-07": 1}},
    }
    curr = {
        "date": "2026-09-08",
        "github": {
            "latest_tag": "v0.1.24",
            "latest_binary_downloads": 1938,
            "all_releases_binary_downloads": 29999,
        },
        "crates_io": {"daily": {"2026-09-08": 2}},
    }
    row = daily_row(prev, curr)
    assert row["github_latest_daily"] == "38", row
    assert row["github_all_binary_daily"] == "99", row
    assert row["crates_downloads"] == "2", row
    first = daily_row(None, curr)
    assert first["github_latest_daily"] == "", first
    new_tag = json.loads(json.dumps(curr))
    new_tag["github"]["latest_tag"] = "v0.1.25"
    new_tag["github"]["latest_binary_downloads"] = 10
    switched = daily_row(prev, new_tag)
    assert switched["github_latest_daily"] == "", switched
    print("self-test ok")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="print full JSON")
    parser.add_argument(
        "--stats-dir",
        metavar="DIR",
        help="write snapshots.jsonl and daily.csv (upsert today's row)",
    )
    parser.add_argument("--self-test", action="store_true", help="run delta logic tests")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0

    try:
        data = collect()
    except (subprocess.CalledProcessError, urllib.error.URLError, urllib.error.HTTPError) as e:
        print(f"failed to fetch stats: {e}", file=sys.stderr)
        return 1

    prev = None
    if args.stats_dir:
        stats_dir = Path(args.stats_dir)
        snapshots = load_snapshots(stats_dir / "snapshots.jsonl")
        prev = previous_snapshot(snapshots, snapshot_date(data))
        write_stats_dir(stats_dir, data)
        print(f"wrote {stats_dir / 'daily.csv'}", file=sys.stderr)

    if args.json:
        json.dump(data, sys.stdout, indent=2)
        print()
    else:
        print_text(data, prev)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
