#!/usr/bin/env python3
"""
Generate DataPrime documentation YAML from the official Coralogix docs JSON.

This script parses the dataprime_docs.json file to extract commands and functions,
and outputs a YAML file that the cx CLI can use for the `cx dataprime` help commands.

Usage:
    # Generate from a local JSON file and write to ~/.cx/dataprime_docs.yaml
    python3 scripts/generate_dataprime_docs.py --input /path/to/dataprime_docs.json

    # Write to stdout (for CI pipelines)
    python3 scripts/generate_dataprime_docs.py --input /path/to/dataprime_docs.json --stdout

    # Write to a custom output path
    python3 scripts/generate_dataprime_docs.py --input /path/to/dataprime_docs.json --output /path/to/output.yaml

The dataprime_docs.json source file can be obtained from the Coralogix documentation
repository or internal sources.

Dependencies: None (uses only Python standard library)
"""

import argparse
import json
import sys
from collections import OrderedDict
from pathlib import Path
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.request import urlopen

DOCS_URL = "https://raw.githubusercontent.com/coralogix/documentation/master/dataprime_docs.json"
DEFAULT_OUTPUT = Path.home() / ".cx" / "dataprime_docs.yaml"


def yaml_escape(s: str) -> str:
    """Escape a string for YAML output."""
    if not s:
        return '""'
    needs_quotes = any(c in s for c in [':', '#', '{', '}', '[', ']', ',', '&', '*', '?', '|', '-', '<', '>', '=', '!', '%', '@', '`', '\n', '"', "'"])
    if needs_quotes or s.startswith(' ') or s.endswith(' '):
        escaped = s.replace('\\', '\\\\').replace('"', '\\"').replace('\n', '\\n')
        return f'"{escaped}"'
    return s


def to_yaml(data: dict[str, Any], indent: int = 0) -> str:
    """Convert a dict to YAML string (simple implementation for our structure)."""
    lines = []
    prefix = "  " * indent
    
    for key, value in data.items():
        if isinstance(value, dict):
            lines.append(f"{prefix}{key}:")
            lines.append(to_yaml(value, indent + 1))
        elif isinstance(value, list):
            items = ", ".join(yaml_escape(str(item)) for item in value)
            lines.append(f"{prefix}{key}: [{items}]")
        elif isinstance(value, str):
            lines.append(f"{prefix}{key}: {yaml_escape(value)}")
        else:
            lines.append(f"{prefix}{key}: {value}")
    
    return "\n".join(lines)


def iterate_pages(item: dict[str, Any], current_path: list[str] | None = None):
    """Recursively iterate through docs structure, yielding (path, page) tuples."""
    if current_path is None:
        current_path = []

    title = item.get("title", "")
    item_type = item.get("type", "")

    if item_type == "section":
        new_path = [*current_path, title]
        for child in item.get("content", []):
            yield from iterate_pages(child, new_path)
    elif item_type == "content":
        yield ([*current_path, title], item)


def parse_dataprime_docs(docs_json: list[dict[str, Any]]) -> dict[str, Any]:
    """Parse the docs JSON and extract commands and functions."""
    commands: OrderedDict[str, dict[str, Any]] = OrderedDict()
    functions: OrderedDict[str, dict[str, Any]] = OrderedDict()

    for item in docs_json:
        title = item.get("title", "")

        if title == "Commands reference":
            for path, page in iterate_pages(item):
                syntax = page.get("syntax")
                description = page.get("description")
                page_id = page.get("id")

                if syntax is None or description is None or page_id is None:
                    continue

                if not page_id.isidentifier():
                    print(
                        f"Warning: Skipping command with invalid ID: {page_id}",
                        file=sys.stderr,
                    )
                    continue

                commands[page_id] = {
                    "description": description,
                    "syntax": syntax,
                    "category": path,
                }

        elif title == "Functions reference":
            for path, page in iterate_pages(item):
                syntax = page.get("syntax")
                description = page.get("description")
                page_id = page.get("id")

                if syntax is None or description is None or page_id is None:
                    continue

                # Fix min and max functions (they have IDs like "aggregation.min")
                if page_id in ("aggregation.min", "aggregation.max"):
                    page_id = page_id.split(".")[1]

                if not page_id.isidentifier():
                    print(
                        f"Warning: Skipping function with invalid ID: {page_id}",
                        file=sys.stderr,
                    )
                    continue

                functions[page_id] = {
                    "description": description,
                    "syntax": syntax,
                    "category": path,
                }

    return {"commands": dict(commands), "functions": dict(functions)}


def validate_counts(docs: dict[str, Any]) -> None:
    """Validate that we have the expected number of commands and functions."""
    num_commands = len(docs["commands"])
    num_functions = len(docs["functions"])

    if not (30 <= num_commands <= 50):
        print(
            f"Warning: Expected 30-50 commands, got {num_commands}",
            file=sys.stderr,
        )

    if not (100 <= num_functions <= 200):
        print(
            f"Warning: Expected 100-200 functions, got {num_functions}",
            file=sys.stderr,
        )

    print(f"Parsed {num_commands} commands and {num_functions} functions", file=sys.stderr)


def download_docs() -> list[dict[str, Any]]:
    """Download the docs JSON from GitHub."""
    print(f"Downloading from {DOCS_URL}...", file=sys.stderr)
    try:
        with urlopen(DOCS_URL) as response:
            return json.loads(response.read().decode("utf-8"))
    except (HTTPError, URLError) as e:
        print(
            f"\nError: Failed to download dataprime_docs.json: {e}\n\n"
            "The source URL may be unavailable or the repository may be private.\n"
            "Please use --input to specify a local dataprime_docs.json file:\n\n"
            "    python3 scripts/generate_dataprime_docs.py --input /path/to/dataprime_docs.json\n",
            file=sys.stderr,
        )
        sys.exit(1)


def load_docs_from_file(path: Path) -> list[dict[str, Any]]:
    """Load docs JSON from a local file."""
    print(f"Loading from {path}...", file=sys.stderr)
    return json.loads(path.read_text())


def main():
    parser = argparse.ArgumentParser(
        description="Generate DataPrime documentation YAML from official docs JSON."
    )
    parser.add_argument(
        "--input",
        type=Path,
        help="Path to local dataprime_docs.json (downloads from GitHub if not provided)",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_OUTPUT,
        help=f"Output path for YAML file (default: {DEFAULT_OUTPUT})",
    )
    parser.add_argument(
        "--stdout",
        action="store_true",
        help="Write to stdout instead of file",
    )
    args = parser.parse_args()

    # Load the docs JSON
    if args.input:
        docs_json = load_docs_from_file(args.input)
    else:
        docs_json = download_docs()

    # Parse and validate
    docs = parse_dataprime_docs(docs_json)
    validate_counts(docs)

    # Generate YAML
    yaml_content = to_yaml(docs)

    # Output
    if args.stdout:
        print(yaml_content)
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(yaml_content)
        print(f"Written to {args.output}", file=sys.stderr)


if __name__ == "__main__":
    main()
