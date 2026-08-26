//! `cx init`: the single guided entry point for onboarding.
//!
//! A thin alias that chains `cx profiles add` and `cx skills install` in order,
//! ending in a success message. It holds no independent profile or skill logic
//! of its own — each step delegates to the command that owns it.
//!
//! **Idempotent by design:** if any profile already exists, the profile step is
//! skipped; if the cx skills are already installed, the skills step is skipped
//! too. Re-running `cx init` therefore never re-prompts, clobbers, or reinstalls
//! an existing setup. To reconfigure a profile use `cx profiles add --force`; to
//! update the skills use `cx skills install`.
//!
//! The profile step only runs on a fresh machine (no profiles yet), so it always
//! creates the *first* profile — which means no name prompt and no "set as
//! default?" question. Two modes, chosen automatically (never via a flag), like
//! `cx profiles add`:
//!
//! * **Quick (interactive)** — bare `cx init` on a terminal. Guided prompts:
//!   OAuth browser login (no auth-method question) and a region prompt.
//!   Credential storage, output format, and label are defaulted
//!   (file / JSON / none) rather than asked; Olly is enabled on the first
//!   profile without asking.
//! * **Advanced (non-interactive)** — the setup is fully specified by flags (or
//!   there is no terminal). The flags make the *profile* step prompt-free; the
//!   skills step still asks its scope question unless `--global-skills`/
//!   `--local-skills` (or `--no-skills`) answers it — so the fully prompt-free
//!   coding-agent one-liner is:
//!   `cx init --url … --api-key $CX_API_KEY --global-skills`. Without a
//!   scope flag and with no terminal, the skills step is skipped with a
//!   warning.

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::Result;
use clap_complete::aot::Shell;
use colored::Colorize;
use inquire::{Select, Text};

use crate::api_client::CxClient;
use crate::banner;
use crate::commands::{completions, profiles, skills};
use crate::config::{self, has_managed_completions, list_profile_names, AuthKind};
use crate::identity;
use crate::region::{region_from_url, RegionMatch};

mod api;

/// Name of the profile init creates on a fresh machine.
const DEFAULT_PROFILE_NAME: &str = "default";

/// Resolved arguments for `cx init`. The credential values come from the
/// shared global flags (`--api-key`, `--region`); the rest are `init`-local
/// flags.
pub struct InitArgs {
    /// `--url`: Coralogix URL the region is derived from.
    pub url: Option<String>,
    /// `--region`: region short-name, alternative to `--url`.
    pub region: Option<String>,
    /// `--api-key` / `CX_API_KEY`.
    pub api_key: Option<String>,
    /// `--oauth`: force OAuth browser login even when an API key is available.
    pub oauth: bool,
    /// Whether to run the skills install step (`--no-skills` turns it off).
    pub install_skills: bool,
    /// `--agent`: agents passed through to the skills installer.
    pub agents: Vec<String>,
    /// Install scope for skills (`--global-skills` / `--local-skills`); `None`
    /// asks (quick) or skips the step (non-interactive).
    pub scope: Option<skills::SkillsScope>,
    /// `--install-completions <shell>`: install shell completions for the given
    /// shell (zsh/bash/fish) without prompting. `None` means the flag was not
    /// passed: interactive runs show a picker (default: don't install), other
    /// non-interactive runs skip the step. Ignored when completions are
    /// already installed.
    pub install_completions: Option<Shell>,
}

/// Orchestrate the onboarding chain: profile → skills → success.
pub async fn run_init(args: InitArgs) -> Result<()> {
    let InitArgs {
        url,
        region,
        api_key,
        oauth,
        install_skills,
        agents,
        scope,
        install_completions,
    } = args;

    // ── Step 1: profile ─────────────────────────────────────────────────────────
    // Idempotent: skip profile setup when the user already has one. A fresh run
    // creates the first (and thus default) profile — no name / set-default
    // prompts to worry about.
    if list_profile_names()?.is_empty() {
        // Auth: quick mode (bare `cx init`, no key) defaults to OAuth without
        // asking the auth-method question; an explicit key uses the API-key
        // path; `--oauth` forces OAuth regardless.
        let oauth = oauth || api_key.is_none();

        println!("Setting up your Coralogix profile...\n");
        profiles::run_add(profiles::AddArgs {
            name: Some(DEFAULT_PROFILE_NAME.to_string()),
            url,
            region,
            api_key,
            oauth,
            force: false,
            set_default: false,
            // `cx init` always enables Olly on the first profile - no opt-out.
            // (`cx profiles add` exposes `--disable-olly`.)
            disable_olly: false,
            quick: true,
        })
        .await?;
    } else {
        println!("A Coralogix profile is already configured - skipping profile setup.");
    }

    // ── Step 2: verify credentials ────────────────────────────────────────────────
    // A deterministic "your setup works" check: one cheap authenticated call
    // (`GET /identity/whoami`) with the resolved credentials, before the slower
    // skills / completions steps so a bad key or wrong region fails fast instead
    // of surfacing on the user's first real query. On failure this returns an
    // error, so `cx init` exits non-zero — the definitive signal a coding agent
    // needs. The profile stays on disk so the user can fix it with
    // `cx profiles add --force` (or by editing the region) and re-run.
    //
    // Verifies the default profile: on a fresh machine that is the profile we
    // just created; on a re-run it is whatever the user already had configured.
    // Custom / BYOC endpoints skip the probe (returns `false`) — see
    // `verify_credentials` — since they may not expose the whoami route.
    let verified = verify_credentials().await?;

    // ── Step 2b: record onboarding (OAuth only, best-effort) ──────────────────────
    // Additionally mark this user as onboarded via the onboarding service
    // (`POST /api/v2/onboarding`, FORGE-876). Credential verification is owned
    // by the `whoami` probe above; this call is purely for recording onboarding.
    // It runs only for OAuth profiles, whose bearer token is user-scoped — the
    // endpoint requires a user token and rejects team API keys. It is fully
    // best-effort and silent: a failure (service not yet deployed to the region,
    // a transient error) must never fail an otherwise-verified setup.
    maybe_report_onboarding().await;

    // ── Step 3: skills ──────────────────────────────────────────────────────────
    // Idempotent, like the profile step: if cx skills are already installed,
    // skip rather than reinstall (`skip_if_installed`). Updating is the
    // explicit command's job (`cx skills install` reinstalls to pull the
    // latest). Detection lives inside `run_install` — its single probe — and
    // init only branches on the reported outcome, so the slow npx spawns are
    // never duplicated on the first-run path.
    //
    // The skills step must never brick onboarding: `skills::run_install` owns
    // the npx/scope diagnostics and fails hard on a missing prerequisite; init
    // downgrades that to a warning and continues, so a working profile still
    // counts as a successful setup.
    if install_skills {
        println!("\nInstalling the cx agent skills for coding agents...");
        match skills::run_install(skills::InstallOptions {
            scope,
            agents,
            skip_if_installed: true,
        }) {
            Ok(skills::InstallOutcome::Installed) => {}
            Ok(skills::InstallOutcome::AlreadyInstalled) => {
                println!(
                    "cx agent skills are already installed - skipping.\n\
                     Update them anytime with `cx skills install`."
                );
            }
            Err(error) => eprintln!("warning: skipped the agent-skills install: {error:#}"),
        }
    }

    // ── Step 4: shell completions ─────────────────────────────────────────────────
    // Idempotent, like the profile and skills steps: if cx already tracks an
    // installed completion, skip the step entirely - no prompt, no rewrite - so
    // re-running `cx init` stays quiet. Add another shell or reinstall later
    // with `cx completions install <shell>`.
    //
    // On a first run: `--install-completions <shell>` installs for that shell at
    // its default path without prompting; otherwise an interactive run shows a
    // picker (default: don't install, plus an "Other" escape hatch for a custom
    // shell/path) and a non-interactive run skips the step.
    if has_managed_completions() {
        println!("\nShell completions are already installed - skipping.");
    } else {
        let completions_choice = if let Some(shell) = install_completions {
            Some((shell, None))
        } else if std::io::stdin().is_terminal() {
            prompt_completions_shell()?
        } else {
            None
        };
        if let Some((shell, path)) = completions_choice {
            println!("\nInstalling {shell} shell completions...");
            if let Err(error) = completions::run_install(shell, path) {
                eprintln!("warning: skipped the shell-completions install: {error:#}");
            }
        }
    }

    // ── Done ────────────────────────────────────────────────────────────────────
    print_success(verified);
    Ok(())
}

/// Interactive picker for the shell-completions step. Offers the standard
/// shells (zsh/bash/fish), a default "don't install" option, and an "other"
/// escape hatch that asks for an explicit shell and path (for shells like
/// PowerShell that have no canonical per-user completion directory, or a custom
/// location). Returns the chosen shell and optional install path, or `None` to
/// skip. Only reached when nothing is installed yet (the caller skips the whole
/// step otherwise).
fn prompt_completions_shell() -> Result<Option<(Shell, Option<PathBuf>)>> {
    const SKIP: &str = "Don't install";
    const OTHER: &str = "Other (specify shell and path)";

    // "Don't install" first so it is the default (starting cursor); the "Other"
    // escape hatch last.
    let options = vec![SKIP, "zsh", "bash", "fish", OTHER];
    let choice = Select::new(
        "Install shell completions? (enables <Tab> completion for cx)",
        options,
    )
    .prompt()?;

    let selection = if choice == "zsh" {
        Some((Shell::Zsh, None))
    } else if choice == "bash" {
        Some((Shell::Bash, None))
    } else if choice == "fish" {
        Some((Shell::Fish, None))
    } else if choice == OTHER {
        // Only shells cx can actually register. Elvish is a clap_complete
        // variant but has no cx adapter, so it's intentionally omitted;
        // PowerShell has no default path, hence the explicit path prompt below.
        let shell = Select::new(
            "Which shell?",
            vec![Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell],
        )
        .prompt()?;
        let path = Text::new("Install path:")
            .with_help_message("Absolute path to write the completion script to.")
            .with_validator(inquire::validator::MinLengthValidator::new(1))
            .prompt()?;
        Some((shell, Some(PathBuf::from(path))))
    } else {
        None
    };
    Ok(selection)
}

/// Run the end-of-setup authenticated health check against the default profile.
///
/// Resolves the default profile (the one just created on a fresh machine, or the
/// user's existing default on a re-run) into a client and runs the shared
/// [`identity::verify_identity`] probe. On failure returns an error naming the
/// likely fix right away, so `cx init` fails fast and exits non-zero.
///
/// Returns `Ok(true)` when the probe ran and succeeded, and `Ok(false)` when it
/// was **skipped for a custom / BYOC / private-link endpoint** (any URL that
/// doesn't resolve to a known Coralogix region): a self-hosted deployment may
/// not expose `GET /identity/whoami` even when its query APIs work, so failing
/// onboarding there would be a false negative. The caller reports the skip
/// instead of the "Connected" box.
async fn verify_credentials() -> Result<bool> {
    let cfg = config::resolve(None, None, None)
        .await
        .map_err(|e| anyhow::anyhow!("could not load the profile to verify it: {e:#}"))?;

    // Only known regions are guaranteed to serve the whoami health-check route.
    if matches!(region_from_url(&cfg.endpoint), RegionMatch::Unresolved) {
        return Ok(false);
    }

    let client = CxClient::new(&cfg.endpoint, &cfg.api_key)?;
    // The identity payload isn't shown; the probe succeeding is the signal.
    identity::verify_identity(&client).await?;
    Ok(true)
}

/// Best-effort onboarding recording for the default profile (FORGE-876).
///
/// Calls `POST /api/v2/onboarding` to mark the user as onboarded, but only when
/// the resolved profile authenticates via OAuth: the endpoint requires a
/// user-scoped token and rejects team API keys, so calling it for an API-key
/// profile would only ever 403. Gated to known regions for the same reason
/// [`verify_credentials`] is — a custom / BYOC endpoint won't expose the route.
///
/// Any failure (service not yet deployed to the region, non-user token, a
/// transient error) is swallowed silently: onboarding is invisible telemetry,
/// and credential verification is already owned by [`verify_credentials`], so a
/// failure here must never affect the outcome of `cx init`.
async fn maybe_report_onboarding() {
    let Ok(cfg) = config::resolve(None, None, None).await else {
        return;
    };
    if cfg.auth_kind != AuthKind::OAuth {
        return;
    }
    if matches!(region_from_url(&cfg.endpoint), RegionMatch::Unresolved) {
        return;
    }
    let Ok(client) = CxClient::new(&cfg.endpoint, &cfg.api_key) else {
        return;
    };
    let _ = api::report_onboarded(&client).await;
}

/// Coralogix brand green, used for the "Connected" confirmation box.
const GREEN: (u8, u8, u8) = (37, 222, 179);

/// Left-aligned green `✓ Connected` box — the definitive "your credentials
/// work" signal at the end of a verified `cx init`.
fn print_connected_box() {
    let (r, g, b) = GREEN;
    // Fixed-width interior so the box borders line up regardless of glyph bytes.
    let interior = " ✓ Connected ";
    let width = interior.chars().count();

    let top = format!("┌{}┐", "─".repeat(width));
    let mid = format!("│{interior}│");
    let bot = format!("└{}┘", "─".repeat(width));
    println!("{}", top.truecolor(r, g, b));
    println!("{}", mid.truecolor(r, g, b));
    println!("{}", bot.truecolor(r, g, b));
}

/// Final success output, printed once at the very end of a clean `cx init`:
/// the left-aligned `--help` logo, then a green box confirming the setup, then
/// the "ready to go" hints. `verified` is the outcome from [`verify_credentials`]:
/// `true` shows the `✓ Connected` box; `false` means verification was skipped
/// (a custom / BYOC endpoint), replaced by a note that the check didn't run.
fn print_success(verified: bool) {
    // The decorative logo is for humans only: gate it on the same condition as
    // the startup banner (interactive TTY, color enabled, not an agent) so a
    // coding agent running `cx init` gets the plain confirmation + hints
    // without the ASCII art. Callers/agents still get the definitive signal.
    if banner::should_show() {
        // Blank line, then the shared `--help` logo (left-aligned, green gradient).
        println!("\n{}", banner::render_logo());
    }
    println!();
    if verified {
        print_connected_box();
    } else {
        // Custom / BYOC endpoint: we didn't probe the identity route, so don't
        // claim a verified setup — just say plainly that the automatic check
        // didn't run (the endpoint may not expose that route at all).
        println!("Profile configured. Skipped the automatic credential check");
    }
    println!();
    println!("Try it out:");
    println!("  cx logs 'source logs | limit 10'");
    println!("  cx schema        # discover every command as JSON");
}
