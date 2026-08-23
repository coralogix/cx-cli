# Setting up cx with an AI agent

This page is the complete flow for installing and configuring the Coralogix CLI (`cx`).
Everything needed is on this page and in the CLI's own `--help` output.

## 1. Install

Ask the user which install method they prefer:

**Homebrew** (macOS):

```bash
brew install coralogix/tap/cx
```

**Install script** (any platform):

```bash
curl -fsSL https://get.coralogix.dev/cli | sh
```

Confirm the binary works:

```bash
cx --version
```

## 2. Configure

`cx` ships a one-step onboarding command that sets up a profile and installs
the agent skills. Ask the user for their Coralogix URL (the address they use
to open Coralogix in the browser, e.g. `https://myteam.app.eu2.coralogix.com`) —
unless they already provided it, in which case don't ask again. Then run:

```bash
cx init --oauth --url <coralogix-url>
```

`--oauth` opens a browser window for the user to log in, so there is no API
key to collect. Run `cx init --help` for the other options supported by the
installed version (API-key login, non-interactive flags for CI, skipping the
skills install).

## 3. Verify

```bash
cx profiles list        # the new profile exists and is the default
cx dashboards catalog   # a cheap read that confirms the API is reachable
```

## Using cx after setup

- `cx --help` — command overview, grouped by domain
- `cx <command> --help` — full syntax and examples for any command
- `cx schema` — the entire command tree as JSON, built for agent consumption
