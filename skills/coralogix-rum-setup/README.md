# Coralogix RUM setup skill

An [Agent Skill](https://agentskills.io) that lets your coding agent (Claude Code, Cursor, Codex and others) add [Coralogix Real User Monitoring](https://coralogix.com/docs/user-guides/rum/getting-started/real-user-monitoring/) to a web or mobile app. Ask your agent to "add error monitoring", "add session replay" or "set up Coralogix RUM", and it:

1. **Detects your stack:** React/Vite, Angular, Vue, Svelte/SvelteKit, Next.js, plain HTML, React Native/Expo, Flutter, iOS or Android, plus your package manager and monorepo layout.
2. **Gets a key:** asks for your region and RUM public key. If you don't have a Coralogix account, it links to the 14-day free trial (no credit card) and keeps working while you sign up.
3. **Installs and initializes the SDK** at your app's entry point, with `application`, `version` and `environment` wired to your build.
4. **Sets privacy defaults:** password, email and phone inputs are masked. Session Replay is off unless you ask for it, and then all inputs are masked.
5. **Sets up source-map upload** in your build or CI, so stack traces show your real code. It uses hidden source maps and deletes them before deploy.
6. **Builds your app and checks for leaked secrets,** then confirms data is arriving or tells you exactly where to look.

## Install

| Agent | Command |
|---|---|
| Claude Code | `claude plugin marketplace add coralogix/cx-skills`, then `claude plugin install coralogix-rum@cx-skills` |
| Cursor | Install the **Coralogix** plugin from the Cursor Marketplace, or `npx skills add coralogix/cx-cli/skills --skill coralogix-rum-setup -a cursor` |
| Codex | `npx skills add coralogix/cx-cli/skills --skill coralogix-rum-setup -a codex` |
| Any of the [40+ supported agents](https://github.com/vercel-labs/skills#supported-agents) | `npx skills add coralogix/cx-cli/skills --skill coralogix-rum-setup` |
| Already using the Coralogix CLI | `cx skills install` (installs every Coralogix skill) |

Add `-g` to the `npx skills` commands to install for all your projects instead of the current one.

Then ask your agent, for example:

- "Add error tracking to this app"
- "Set up Coralogix RUM with session replay"
- "Add crash reporting to my React Native app"

## With or without the Coralogix CLI

The skill works on its own. If the [Coralogix CLI](https://github.com/coralogix/cx-cli) (`cx`) is installed and signed in, the agent can also:
- find or create the RUM key for you (it asks first);
- query incoming RUM events to confirm the setup works.

`cx init --signup` creates a new account through your browser and connects the CLI in one step.

## Privacy and security

- **Your agent never creates an account or types credentials for you.** Signup happens in your browser.
- **The RUM public key** is public by design, because it ships in your client bundle. It still goes into your project's env config, not hard-coded in source.
- **The source-map upload key is secret.** The skill only references it as a CI secret (`CORALOGIX_SOURCEMAP_API_KEY`). It never writes it to the repo and never asks you to paste it into the chat.
- **No extra tracking.** The skill adds nothing to your app beyond the standard Coralogix SDK. The signup link it shows carries UTM parameters, so Coralogix can count signups that came from coding agents.
- **No pushing Coralogix.** The skill only activates when you ask for monitoring. It doesn't touch other monitoring SDKs you already use.

## Source

This skill is maintained in [`coralogix/cx-cli`](https://github.com/coralogix/cx-cli/tree/master/skills/coralogix-rum-setup), which also ships it with the Coralogix CLI. Copies in other Coralogix repos are synced from there. Licensed under Apache-2.0.
