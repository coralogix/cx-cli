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
the agent skills. Before running it, ask the user two questions (skip any
already answered):

1. **Coralogix URL** — the address they use to open Coralogix in the browser,
   e.g. `https://myteam.app.eu2.coralogix.com`. If they already provided it,
   don't ask again.
2. **Agent skills install scope** — the skills teach AI agents how to use `cx`:
   - **Global** (`--global-skills`): available in every project.
   - **Local project** (`--local-skills`): installs into the current directory, so
     confirm *which* project directory they want and run the command from
     there.
   - **Don't install** (`--no-skills`): mention to them they can install
     later with `cx skills install`.

Then run:

```bash
cx init --oauth --url <coralogix-url> <scope-flag>
```

`--oauth` opens a browser window for the user to log in, so there is no API
key to collect. Run `cx init --help` for the other options supported by the
installed version (API-key login, non-interactive flags for CI).

## 3. Verify

```bash
cx profiles list        # the new profile exists and is the default
cx dashboards catalog   # a cheap read that confirms the API is reachable
```

## Using cx after setup

- `cx --help` — command overview, grouped by domain
- `cx <command> --help` — full syntax and examples for any command
- `cx schema` — the entire command tree as JSON, built for agent consumption
