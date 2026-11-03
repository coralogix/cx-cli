# Set up the Coralogix CLI

This page sets up the Coralogix CLI (`cx`) for one of two people:

- **New to Coralogix:** no account yet. Your agent installs `cx`, signs you up for a new Coralogix account and team, and creates a key so you can start sending data.
- **New to the CLI:** your team already uses Coralogix and is sending data, and you just want `cx` on your machine. Your agent installs `cx` and signs you in to your existing team.

Either way, paste this into Claude Code, Cursor, Codex, or any other coding agent:

```text
Set me up with the Coralogix CLI by following https://coralogix.com/docs/cli
```

The rest of this page is written for the agent.

---

## Instructions for the agent

Take the user from nothing installed to a signed-in `cx` profile. The user is one of two kinds, and Step 3 finds out which:

- **New Coralogix customer** (no account): sign them up with `cx init --signup` (Step 4b), then create a Send-Your-Data key so they can start sending data (Step 6).
- **New CLI user on an existing Coralogix environment** (their team already has an account, and usually data flowing): sign them in to that team (Step 4a). Don't push them through data setup; offer the key in Step 6 only if they want to send new data.

Follow the steps in order.

Ground rules:

- **Ask, don't guess, the region.** For a new account it's permanent.
- **Answer every `cx init` question with flags** so it never stops at a prompt, and pass the sign-in URL from stderr on to the user at once.
- **The browser part belongs to the user.** Wait for `cx init` to exit. Never fill in the signup form, solve the reCAPTCHA, or ask for a password.
- **Never overwrite a `cx` profile** without the user's consent.
- **Create API keys only after the user approves**, and treat key values as secrets.

### Step 1: Install the CLI

If `cx --version` already works, upgrade instead (`brew upgrade cx`, or rerun the install script): older versions lack `cx init --signup`. Otherwise detect the OS (`uname -s`; on Windows, `$env:OS` in PowerShell) and ask the user which method they prefer when more than one fits.

**macOS** (Homebrew, recommended) or **Linux** with Homebrew:

```bash
brew install cx
```

**macOS / Linux** install script (x86_64 and arm64, latest release):

```bash
curl -fsSL https://get.coralogix.dev/cli | sh
```

**Windows** (PowerShell; the install script doesn't support Windows). The zip holds a single `cx.exe`:

```powershell
$dir = "$env:LOCALAPPDATA\Programs\cx"
New-Item -ItemType Directory -Force -Path $dir | Out-Null
Invoke-WebRequest https://github.com/coralogix/cx-cli/releases/latest/download/cx-x86_64-pc-windows-msvc.zip -OutFile "$env:TEMP\cx.zip"
Expand-Archive "$env:TEMP\cx.zip" -DestinationPath $dir -Force
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$dir*") { [Environment]::SetEnvironmentVariable('Path', "$userPath;$dir", 'User') }
$env:Path += ";$dir"
```

**Any OS with Rust:** `cargo install coralogix-cli`. If a security policy blocks piped scripts, use the signed release archives and checksums at https://github.com/coralogix/cx-cli/releases/latest.

Confirm with `cx --version`. `PATH` changes may need a new shell.

### Step 2: Check for an existing profile

```bash
cx profiles list
```

`cx init` only creates a profile on a machine with **none**. If one exists, it skips the profile step and ignores `--region`/`--url`/`--oauth`/`--signup`. Ask the user whether to keep using it.

- **Keep it:** ask the Step 3 skills question, skip Step 4, and run `cx init <skills-flag> [--install-completions <shell>]` to install them. Then continue at Step 5.
- **Another team or a new account:** ask the Step 3 questions as usual, but instead of the `cx init` command in Step 4, create the profile with `cx profiles add`. It takes only the profile flags, not the skills or completions ones:

  ```bash
  cx profiles add <name> --oauth --region <region> --set-default    # existing account (Step 4a)
  cx profiles add <name> --signup --region <region> --set-default   # new account (Step 4b)
  ```

  The same waits and URL hand-off as in Step 4 apply. Then run `cx init <skills-flag> [--install-completions <shell>]`: it skips the profile step and installs the rest. Pick a `<name>` that isn't taken; never pass `--force` without consent.

### Step 3: Ask the setup questions

Ask these up front, skipping anything the user already told you:

1. **Do you (or your team) already use Coralogix?** If yes, they're a new CLI user on an existing environment: Step 4a. If no, they're a new Coralogix customer: Step 4b. If they were invited to their team's Coralogix but haven't accepted the invitation yet, have them accept it first, then use Step 4a. Don't sign them up for a separate account.
2. **Region.** For an existing account, the easiest answer is the URL they open Coralogix with (e.g. `https://myteam.app.eu2.coralogix.com`): pass it as `--url` and `cx` works out the region. For a new account the region is permanent and decides where data is stored. Offer the regions that accept new accounts:

   | Region | Location | Coralogix domain (for shippers) |
   |---|---|---|
   | `us2` | United States (AWS) | `us2.coralogix.com` |
   | `us3` | United States (GCP) | `us3.coralogix.com` |
   | `eu2` | Europe | `eu2.coralogix.com` |
   | `ap1` | India | `ap1.coralogix.com` |
   | `ap2` | Singapore | `ap2.coralogix.com` |
   | `ap3` | Indonesia | `ap3.coralogix.com` |

   `us1` and `eu1` don't accept new accounts (offer `us2`/`eu2` instead), but work for signing in to existing ones. If the user is unsure, suggest the region closest to their systems, or the one their data-residency rules require.
3. **Where to install the cx agent skills,** which teach coding agents to use `cx`: everywhere (`--global-skills`), this project only (`--local-skills`, run from that project's directory), or not at all (`--no-skills`). The install needs Node.js (`npx`). The commands below write the chosen flag as `<skills-flag>`.

**Install shell completions without asking.** Take the shell from `basename "$SHELL"` and pass `--install-completions <shell>` when it's `zsh`, `bash` or `fish`. For any other shell, and on Windows, leave the flag out: it only supports those three, and without it `cx init` skips completions when there's no terminal. Also leave it out if the user says they don't want completions.

### Step 4a: Existing Coralogix environment: sign in

```bash
cx init --oauth --region <region> <skills-flag> --install-completions <shell>
```

Use `--url <their-coralogix-url>` instead of `--region` if that's what they gave you. With every question answered by flags, `cx init` runs without prompts:

- It prints `Sign in by visiting:` and a URL on **stderr**, tries to open the browser, and **waits up to 5 minutes** for approval. Run it in the background (or with a timeout of at least 6 minutes) and give the user the URL immediately, in case the browser didn't open. Keep the process running: sign-in completes through a `localhost` callback in it.
- If the user belongs to several teams, they pick one on the approval page.
- If the user has a **Team Key or Personal Key** (not a Send-Your-Data key) and prefers no browser, run `cx init --region <region> --api-key "$CX_API_KEY" <skills-flag>`. Never ask them to paste the key into the chat; have them export it in their shell.

### Step 4b: New to Coralogix: sign up

```bash
cx init --signup --region <region> <skills-flag> --install-completions <shell>
```

`--signup` opens Coralogix's signup page instead of the sign-in page, and creates the profile once the new team exists. It **waits up to 30 minutes**, so run it in the background with a timeout of at least 31 minutes. Give the user the stderr URL right away, with these instructions:

1. Fill in the signup form and complete the reCAPTCHA.
2. Open the verification email from Coralogix and click its link (check spam).
3. Set a password, then create a team when asked.
4. Approve the CLI's access on the page that follows.

If it times out after the user finished signing up, rerun the **same command without `--signup`** (Step 4a).

### Step 5: Confirm it worked

`cx init` exits 0 and prints a `✓ Connected` box after a live credential check. A non-zero exit means the profile was saved but the check failed, usually because the region doesn't match the account. With the user's consent, fix it with `cx profiles add default --oauth --region <right-region> --force`. Then check that `cx profiles list` shows the profile as the default.

### Step 6: Create a Send-Your-Data key

**New Coralogix customer:** always offer this, since a new team has no data yet. **New CLI user on an existing environment:** their team is likely sending data already, so only offer it if they say they want to send new data (e.g. a new service); otherwise skip to Step 7. Shippers (OpenTelemetry Collector, Fluent Bit, SDKs) authenticate with a Send-Your-Data key. It isn't the credential `cx` uses.

The commands below use `jq` to keep key values out of the output. If `jq` isn't installed, install it or read the JSON without echoing the `value` fields back.

1. **Reuse an existing key if there is one:**

   ```bash
   cx iam api-keys send-data-keys -o json | jq 'map(del(.value))'
   ```

   This lists every ingest-only key with its `presets`. Reuse a key only if `active` is `"yes"`, `presets` is exactly `["SendData"]` (`SendRUMData` keys are for RUM only), **and** `hashed` is `false`. A hashed key's value can't be read back: offer it only if the user still has the value saved, otherwise create a new key. New teams usually have none.

2. **Find the team ID:** `cached_team_id` in `~/.cx/profiles/<profile>.toml` if present (older profiles lack it). Otherwise use the `owner.teamId` of a team-owned key from `cx iam api-keys list -o json | jq 'map(del(.value))'`, or ask the user.

3. **Show the key definition and get approval.** `cx iam` writes need `--yes`, which you may pass only after the user explicitly approves.

   ```json
   {
     "name": "send-your-data",
     "owner": { "teamId": <team-id> },
     "keyPermissions": { "presets": ["SendData"], "permissions": [] },
     "hashed": false
   }
   ```

   - `SendData` allows sending logs, metrics, spans, profiles and related data, and nothing else. Don't add other presets or permissions.
   - `hashed: false` keeps the value viewable later under **Settings → API Keys**. With `true` it's shown only once. Let the user choose.

4. **Create it** once approved:

   ```bash
   echo '{"name":"send-your-data","owner":{"teamId":<team-id>},"keyPermissions":{"presets":["SendData"],"permissions":[]},"hashed":false}' \
     | cx iam api-keys create --from-file - --yes -o json
   ```

   The response's `value` is the key. Verify it with `cx iam api-keys get <key_id> -o json | jq '.keyInfo | del(.value)'`: expect `active: true`, the team's `owner.teamId`, and a single `SendData` preset.

5. **Hand it over.** Give the user the key, marked as a secret, plus their **Coralogix domain** from the Step 3 table. It's always `<region>.coralogix.com`, e.g. `us2.coralogix.com`. Shippers need both; the OTLP endpoint is `ingress.<coralogix domain>:443`. Suggest keeping the key in a secret manager or environment variable, never in a committed file.

### Step 7: Next steps

- **New Coralogix customer:** find the guide for their stack with `cx docs search "<their stack> send data"`, then `cx docs fetch <result>`. Once data arrives, query with `--tier frequent`, e.g. `cx logs --tier frequent 'source logs | limit 10'`: new accounts have no archive, so the default archive tier returns `[]`.
- **New CLI user on an existing environment:** show them their data right away, e.g. `cx logs 'source logs | limit 10'`. If that returns `[]`, retry with `--tier frequent`.
- **Both:** if the skills were installed, the coding agent can now investigate data, build dashboards and set up alerts with `cx`. `cx schema` lists every command.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `no region — pass --url or --region` | There's no terminal to prompt on. Ask the user (Step 3). |
| `A Coralogix profile is already configured - skipping profile setup` | A profile exists and the flags were ignored. Use `cx profiles add` (Step 2). |
| `OAuth login timed out after 5 minutes` | Approval wasn't completed. Rerun Step 4a and give the user the new URL. |
| `OAuth sign-up timed out after 30 minutes` | If signup finished, rerun without `--signup`. Otherwise start Step 4b again. |
| The signup link lands on a login page | The region is closed to new accounts (`us1`/`eu1`). Pick one from the Step 3 table. |
| `cx init` exits non-zero after sign-in | The credential check failed, usually because of the wrong region. See Step 5. |
| `warning: skipped the agent-skills install` | Node.js/`npx` is missing. The CLI still works; install Node and run `cx skills install` later. |
| `cx iam api-keys create` asks for confirmation | Non-interactive runs need `--yes`, passed only after the user approves. |
| `Permission denied` on `get`/`create` | The user's role lacks `DATA-INGEST-API-KEYS:MANAGE`. Ask an admin, or create the key in **Settings → API Keys** (type Send Your Data, preset `SendData`). |
