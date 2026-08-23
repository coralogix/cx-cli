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
//!   OAuth browser login (no auth-method question), a region prompt, and the
//!   first-profile safety questions. Credential storage, output format, and
//!   label are defaulted (file / JSON / none) rather than asked.
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
use clap::ValueEnum;
use clap_complete::aot::Shell;
use inquire::{Select, Text};

use crate::commands::{completions, profiles, skills};
use crate::config::list_profile_names;

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
    /// `--olly-enabled`: enable the Olly AI assistant (`olly ask`) on the first
    /// profile without prompting. Interactive runs ask; other non-interactive
    /// runs leave it off.
    pub olly_enabled: bool,
    /// `--install-completions <shell>`: install shell completions for the given
    /// shell (zsh/bash/fish) without prompting. `None` means the flag was not
    /// passed: interactive runs show a picker (default: don't install), other
    /// non-interactive runs skip the step.
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
        olly_enabled,
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
            olly_enabled,
            quick: true,
        })
        .await?;
    } else {
        println!("A Coralogix profile is already configured - skipping profile setup.");
    }

    // ── Step 2: skills ──────────────────────────────────────────────────────────
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

    // ── Step 3: shell completions ─────────────────────────────────────────────────
    // `--install-completions <shell>` installs for that shell using its default
    // path, without prompting. With no flag, an interactive run shows a picker
    // (default: don't install, plus an "Other" escape hatch for a custom
    // shell/path) and a non-interactive run skips the step.
    let completions_choice = if let Some(shell) = install_completions {
        Some((shell, None))
    } else if std::io::stdin().is_terminal() {
        prompt_completions_shell()?
    } else {
        None
    };
    if let Some((shell, path)) = completions_choice {
        // Idempotent: if the chosen shell's completions are already installed and
        // we'd write to the default location, skip rather than rewrite them, so
        // re-running `cx init` is quiet. An explicit "Other" path is always
        // honored, and `cx completions install`/`refresh` force a rewrite.
        if path.is_none() && completions::installed_shells().contains(&shell) {
            println!("\n{shell} shell completions are already installed - skipping.");
        } else {
            println!("\nInstalling {shell} shell completions...");
            if let Err(error) = completions::run_install(shell, path) {
                eprintln!("warning: skipped the shell-completions install: {error:#}");
            }
        }
    }

    // ── Done ────────────────────────────────────────────────────────────────────
    print_success();
    Ok(())
}

/// Interactive picker for the shell-completions step. Offers the standard
/// shells (zsh/bash/fish), a default "don't install" option, and an "other"
/// escape hatch that asks for an explicit shell and path (for shells like
/// PowerShell that have no canonical per-user completion directory, or a custom
/// location). Returns the chosen shell and optional install path, or `None` to
/// skip. Whether the chosen shell is already installed is handled by the caller
/// (it skips the actual install), so every shell is always listed.
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
        let shell = Select::new("Which shell?", Shell::value_variants().to_vec()).prompt()?;
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

fn print_success() {
    println!("\ncx is ready to go.");
    println!("Try it out:");
    println!("  cx logs 'source logs | limit 10'");
    println!("  cx schema        # discover every command as JSON");
}
