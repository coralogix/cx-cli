//! `cx init`: the single guided entry point for onboarding.
//!
//! A thin alias that chains `cx profiles add` and `cx skills install` in order,
//! ending in a success message. It holds no independent profile or skill logic
//! of its own — each step delegates to the command that owns it.
//!
//! **Idempotent by design:** if any profile already exists, the profile step is
//! skipped entirely and init drops straight to the skills install. Re-running
//! `cx init` therefore never re-prompts or clobbers an existing setup; to
//! reconfigure a profile, use `cx profiles add --force`.
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
//!   there is no terminal). Prompt-free; the coding-agent one-liner:
//!   `cx init --url … --api-key $CX_API_KEY`.

use anyhow::Result;

use crate::commands::{profiles, skills};
use crate::config::list_profile_names;

/// Default profile name created on a fresh machine when `--profile` is absent.
const DEFAULT_PROFILE_NAME: &str = "default";

/// Resolved arguments for `cx init`. The profile/credential values come from
/// the shared global flags (`--profile`, `--api-key`, `--region`); the rest are
/// `init`-local flags.
pub struct InitArgs {
    /// Profile name (`--profile`); defaults to "default" when absent.
    pub profile: Option<String>,
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
    /// Install scope for skills (`--global` / `--local`); `None` asks (quick)
    /// or skips the step (non-interactive).
    pub scope: Option<skills::SkillsScope>,
}

/// Orchestrate the onboarding chain: profile → skills → success.
pub async fn run_init(args: InitArgs) -> Result<()> {
    let InitArgs {
        profile,
        url,
        region,
        api_key,
        oauth,
        install_skills,
        agents,
        scope,
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
            name: Some(profile.unwrap_or_else(|| DEFAULT_PROFILE_NAME.to_string())),
            url,
            region,
            api_key,
            oauth,
            force: false,
            set_default: false,
            quick: true,
        })
        .await?;
    } else {
        println!(
            "A Coralogix profile is already configured - skipping profile setup.\n\
             Manage profiles with `cx profiles add` (use --force to reconfigure)."
        );
    }

    // ── Step 2: skills ──────────────────────────────────────────────────────────
    // The skills step must never brick onboarding. `skills::run_install` owns
    // the npx/scope diagnostics and fails hard on a missing prerequisite; init
    // downgrades that to a warning and continues, so a working profile still
    // counts as a successful setup. The error already names the fix (install
    // Node.js, pass --global/--local, or retry the manual command).
    if install_skills {
        println!("\nInstalling the cx agent skills for coding agents...");
        if let Err(error) = skills::run_install(skills::InstallOptions { scope, agents }) {
            eprintln!("warning: skipped the agent-skills install: {error:#}");
        }
    }

    // ── Done ────────────────────────────────────────────────────────────────────
    print_success();
    Ok(())
}

fn print_success() {
    println!("\ncx is ready to go.");
    println!("Try it out:");
    println!("  cx logs 'source logs | limit 10'");
    println!("  cx schema        # discover every command as JSON");
}
