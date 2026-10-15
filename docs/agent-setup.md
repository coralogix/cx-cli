# Setting up cx with an AI agent

This page is the complete flow for installing and configuring the Coralogix CLI (`cx`).
Everything needed is on this page and in the CLI's own `--help` output.

## 1. Install

**If `cx` is already installed** (`cx --version` succeeds), upgrade it instead
of skipping this step — older versions can't complete setup. Use the method it
was installed with (a `cx` under the Homebrew prefix came from Homebrew):

```bash
brew upgrade coralogix/tap/cx                  # Homebrew
curl -fsSL https://get.coralogix.dev/cli | sh  # install script: re-run it
```

Otherwise, ask the user which install method they prefer:

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

`cx` ships a one-step onboarding command that sets up a profile, installs
the agent skills, and installs shell completions. Before running it, ask the
user three questions (skip any already answered):

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
3. **Shell completions** — whether they want `<Tab>` completion for `cx`,
   and for which shell (`zsh`, `bash`, or `fish` — usually inferable from
   `$SHELL`):
   - **Yes** (`--install-completions <shell>`): installs without prompting.
   - **No** (omit the flag): without a terminal the step is skipped; mention
     they can install later with `cx completions install <shell>`.

Then run these two commands, in this order — **even if `cx` is already
configured**. Don't skip them because a profile exists: the first one signs the
user in to *this* Coralogix URL and links the setup to their account.

```bash
cx profiles add <team> --oauth --url <coralogix-url> --force --set-default < /dev/null
cx init <scope-flag> [--install-completions <shell>]
```

- `<team>` is the first label of the URL's host, e.g. `myteam` for
  `https://myteam.app.eu2.coralogix.com`. Naming the profile after the team
  leaves the user's other profiles untouched; `--force` only replaces an
  earlier profile for the same team.
- `--oauth` opens a browser window for the user to log in, so there is no API
  key to collect. `< /dev/null` keeps it from stopping at interactive prompts.
- `--set-default` makes it the default profile. If the user already had
  profiles and wants to keep their current default, drop the flag — but
  `cx init` then checks that existing default profile's credentials instead.
- `cx init` then installs the agent skills and shell completions; it keeps the
  profile you just created.

Run `cx profiles add --help` and `cx init --help` for the other options
supported by the installed version (API-key login, non-interactive flags for
CI).

## 3. Verify

```bash
cx profiles list                          # the <team> profile exists
cx dashboards catalog --profile <team>    # a cheap read that confirms the API is reachable
```

## Using cx after setup

- `cx --help` — command overview, grouped by domain
- `cx <command> --help` — full syntax and examples for any command
- `cx schema` — the entire command tree as JSON, built for agent consumption
