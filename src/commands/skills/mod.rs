//! Skills install step: shells out to the vercel-labs `skills` npx installer
//! to install the cx agent skills bundle (`coralogix/cx-cli/skills`).
//!
//! Owns no skill/agent logic of its own beyond driving the one scope
//! question (global vs local) and passing flags through to the installer.

use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::process::Command;

use anyhow::{bail, Context, Result};
use inquire::Select;
use serde::Deserialize;

/// The source passed to `skills add`. Points at the repo's user-facing
/// `skills/` subdirectory, not the repo root: `--skill '*'` installs every
/// `SKILL.md` the installer discovers, and the repo root also carries the
/// contributor-only dev skills under `.claude/skills/` (add-command,
/// run-tests, ...) which must never reach end users.
pub const SKILLS_SOURCE: &str = "coralogix/cx-cli/skills";

/// The source the installer records in its lock file for skills installed from
/// this repo. It stores the repo slug without the subdir, so already-installed
/// detection matches on this rather than [`SKILLS_SOURCE`].
const INSTALLED_SOURCE: &str = "coralogix/cx-cli";

/// The command a user can run by hand when cx fails the install.
const MANUAL_INSTALL_CMD: &str = "npx skills add coralogix/cx-cli/skills";

/// Where to review the installed skills (and their risk assessments). Shown in
/// the compact summary in place of the installer's own security-risk table,
/// which we suppress along with the rest of its verbose output.
const SKILLS_REVIEW_URL: &str = "https://skills.sh/coralogix/cx-cli";

/// Where the installer puts skills: the user's home (`~/`) or the project (`./`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillsScope {
    Global,
    Local,
}

pub struct InstallOptions {
    /// Scope from `--global`/`--local`; `None` asks interactively.
    pub scope: Option<SkillsScope>,
    /// Agents passed through to the installer's `-a` (overrides auto-detect).
    pub agents: Vec<String>,
}

/// The pre-flight gate run_install evaluates before touching the installer,
/// kept pure so both failure branches are unit-testable without a terminal.
#[derive(Debug, PartialEq, Eq)]
enum Preflight {
    Fail(String),
    /// `scope` is `None` when it still needs to be asked interactively.
    Proceed {
        scope: Option<SkillsScope>,
    },
}

fn preflight(
    npx_available: bool,
    scope: Option<SkillsScope>,
    stdin_is_terminal: bool,
) -> Preflight {
    if !npx_available {
        return Preflight::Fail(
            "Node.js/npx is required to install the cx agent skills.\n\
             Install Node.js (https://nodejs.org) and rerun, or skip the skills install."
                .to_string(),
        );
    }
    if scope.is_none() && !stdin_is_terminal {
        return Preflight::Fail("no skills install scope - pass --global or --local".into());
    }
    Preflight::Proceed { scope }
}

/// Install with at most one question (scope), then a fully
/// non-interactive `npx skills add` run.
pub fn run_install(opts: InstallOptions) -> Result<()> {
    let scope = match preflight(npx_available(), opts.scope, std::io::stdin().is_terminal()) {
        Preflight::Fail(message) => bail!(message),
        Preflight::Proceed { scope } => match scope {
            Some(scope) => scope,
            None => ask_scope()?,
        },
    };

    // Informational only: an explicit install reinstalls to update. Detection
    // is scoped to the scope we're about to install into, so skills present in
    // the other scope don't muddy the message.
    let installed = installed_cx_skills(scope);
    if !installed.is_empty() {
        println!(
            "cx agent skills already installed ({}) - reinstalling to update.",
            installed_summary(&installed)
        );
    }

    let args = build_install_args(scope, &opts.agents);

    // The `-y` installer runs fully non-interactively, so it needs no terminal
    // and its verbose TUI (banner, per-skill table, risk matrix) is pure noise.
    // Capture it and replace it with a one-line summary; only surface the raw
    // output on failure, where the error detail matters. The user-driven
    // interactive walk (`run_advanced_install`) keeps full passthrough.
    println!("Installing cx agent skills…");

    let output = Command::new(npx_program())
        .args(&args)
        .output()
        .context("failed to run npx")?;
    if !output.status.success() {
        // Show the installer's own diagnostics before the retry hint. Node
        // CLIs report errors on either stream, so surface both.
        for captured in [&output.stdout, &output.stderr] {
            let text = String::from_utf8_lossy(captured);
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                eprintln!("{trimmed}");
            }
        }
        bail!(
            "skills installer exited with {}.\n\
             You can retry manually with: {MANUAL_INSTALL_CMD}",
            output.status
        );
    }

    print_install_summary(scope);
    Ok(())
}

/// The compact success line shown after a non-interactive install, in place of
/// the installer's verbose output. Keeps the installer's own "review before
/// use" nudge alive, since we've hidden its security-risk table.
fn print_install_summary(scope: SkillsScope) {
    let scope_label = match scope {
        SkillsScope::Global => "global",
        SkillsScope::Local => "local",
    };
    // Re-query so the count reflects what's actually on disk now.
    let count = installed_cx_skills(scope).len();
    if count > 0 {
        println!(
            "✓ Installed {count} cx agent skills ({scope_label}). \
             Review before use: {SKILLS_REVIEW_URL}"
        );
    } else {
        // Installer succeeded but detection came up empty (e.g. an older
        // installer without `--json`); still confirm and point to review.
        println!(
            "✓ cx agent skills installed ({scope_label}). \
             Review before use: {SKILLS_REVIEW_URL}"
        );
    }
}

/// Advanced install: run the raw installer with no flags and let the user
/// walk its full interactive flow (skill/agent selection, scope, method).
pub fn run_advanced_install() -> Result<()> {
    println!("Running the skills installer interactively: {MANUAL_INSTALL_CMD}");
    let status = Command::new(npx_program())
        .args(["skills", "add", SKILLS_SOURCE])
        .status()
        .context(
            "failed to run npx - Node.js is required for the skills install \
             (https://nodejs.org)",
        )?;
    if !status.success() {
        bail!("skills installer exited with {status}");
    }
    Ok(())
}

/// Best-effort check for whether cx skills are already installed, used by
/// `cx init` to stay idempotent and skip the skills step on a re-run. When
/// `scope` is `Some`, only that scope is checked (an explicit `--global`/
/// `--local`); when `None` (init hasn't asked yet), either scope counts.
///
/// Returns `false` when npx is unavailable or detection fails, so `init` falls
/// through to a normal install rather than skipping on a bad signal. Detection
/// shares `installed_cx_skills`' limitation: copy-installed skills aren't seen.
pub fn cx_skills_present(scope: Option<SkillsScope>) -> bool {
    if !npx_available() {
        return false;
    }
    match scope {
        Some(scope) => !installed_cx_skills(scope).is_empty(),
        None => {
            !installed_cx_skills(SkillsScope::Global).is_empty()
                || !installed_cx_skills(SkillsScope::Local).is_empty()
        }
    }
}

// ── Building blocks ───────────────────────────────────────────────────────────

/// `npx` resolves through a `.cmd` shim on Windows; `Command` needs the
/// real file name there.
fn npx_program() -> &'static str {
    if cfg!(windows) {
        "npx.cmd"
    } else {
        "npx"
    }
}

fn npx_available() -> bool {
    Command::new(npx_program())
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// One entry of `skills ls --json` output; extra fields are ignored.
#[derive(Deserialize)]
struct LsSkill {
    name: String,
    #[serde(default)]
    source: Option<String>,
}

/// cx skills already installed in `scope`, according to the installer itself
/// (`npx skills ls [-g] --json`). A skill counts as ours when its lock-file
/// source is [`INSTALLED_SOURCE`]. Any failure (old installer without `--json`,
/// unparseable output) counts as nothing installed, so the install proceeds.
///
/// Copy installs (`--copy`) drop the lock-file source and so are not detected;
/// the installer's default is symlink, which preserves it.
fn installed_cx_skills(scope: SkillsScope) -> Vec<String> {
    let mut cmd = Command::new(npx_program());
    cmd.args(["-y", "skills", "ls"]);
    if scope == SkillsScope::Global {
        cmd.arg("-g");
    }
    cmd.arg("--json");
    let Ok(output) = cmd.output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(skills) = serde_json::from_slice::<Vec<LsSkill>>(&output.stdout) else {
        return Vec::new();
    };
    skills
        .into_iter()
        .filter(|skill| skill.source.as_deref() == Some(INSTALLED_SOURCE))
        .map(|skill| skill.name)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Arguments for the fully non-interactive install.
///
/// The leading `-y` is npx's own auto-approve (first-run package download
/// prompt); the trailing `-y` is the installer's skip-all-confirmations.
fn build_install_args(scope: SkillsScope, agents: &[String]) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-y".into(),
        "skills".into(),
        "add".into(),
        SKILLS_SOURCE.into(),
        "--skill".into(),
        "*".into(),
        "-y".into(),
    ];
    if scope == SkillsScope::Global {
        args.push("-g".into());
    }
    if !agents.is_empty() {
        args.push("-a".into());
        args.extend(agents.iter().cloned());
    }
    args
}

/// e.g. "3 skills: cx-alerts, cx-dashboards, ..."
fn installed_summary(installed: &[String]) -> String {
    const SHOWN: usize = 3;
    let names = installed
        .iter()
        .take(SHOWN)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let suffix = if installed.len() > SHOWN { ", ..." } else { "" };
    format!("{} skills: {names}{suffix}", installed.len())
}

fn ask_scope() -> Result<SkillsScope> {
    const GLOBAL: &str = "Global (~/) - available in every project";
    const LOCAL: &str = "Local (./) - this project only";
    let chosen = Select::new(
        "Where should the agent skills be installed?",
        vec![GLOBAL, LOCAL],
    )
    .with_help_message(
        "Skills teach coding agents (Claude Code, Cursor, Codex, ...) how to use cx.",
    )
    .prompt()?;
    Ok(if chosen == GLOBAL {
        SkillsScope::Global
    } else {
        SkillsScope::Local
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_args_local_scope() {
        let args = build_install_args(SkillsScope::Local, &[]);
        assert_eq!(
            args,
            ["-y", "skills", "add", SKILLS_SOURCE, "--skill", "*", "-y"]
        );
    }

    #[test]
    fn install_args_global_scope() {
        let args = build_install_args(SkillsScope::Global, &[]);
        assert_eq!(
            args,
            [
                "-y",
                "skills",
                "add",
                SKILLS_SOURCE,
                "--skill",
                "*",
                "-y",
                "-g"
            ]
        );
    }

    #[test]
    fn install_args_pass_agents_through() {
        let args = build_install_args(
            SkillsScope::Global,
            &["claude-code".to_string(), "cursor".to_string()],
        );
        assert_eq!(
            args,
            [
                "-y",
                "skills",
                "add",
                SKILLS_SOURCE,
                "--skill",
                "*",
                "-y",
                "-g",
                "-a",
                "claude-code",
                "cursor"
            ]
        );
    }

    #[test]
    fn summary_truncates_long_skill_lists() {
        let skills: Vec<String> = ["cx-a", "cx-b", "cx-c", "cx-d"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            installed_summary(&skills),
            "4 skills: cx-a, cx-b, cx-c, ..."
        );
        assert_eq!(installed_summary(&skills[..2]), "2 skills: cx-a, cx-b");
    }

    #[test]
    fn ls_json_matches_only_our_source_ignoring_extra_fields() {
        // Our skills carry the repo source; a same-named skill from another
        // source and a copy-installed skill (null source) are not ours.
        let json = r#"[
            {"name": "cx-alerts", "path": "/p", "scope": "global",
             "agents": ["Claude Code"], "source": "coralogix/cx-cli",
             "sourceUrl": null, "sourceType": "github"},
            {"name": "cx-other", "source": "someone/else"},
            {"name": "cx-copied", "source": null}
        ]"#;
        let skills: Vec<LsSkill> = serde_json::from_str(json).unwrap();
        let ours: Vec<&str> = skills
            .iter()
            .filter(|s| s.source.as_deref() == Some(INSTALLED_SOURCE))
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(ours, ["cx-alerts"]);
    }

    // ── preflight: both failure branches and the two proceed branches ─────────

    #[test]
    fn preflight_no_npx_fails_actionably() {
        let Preflight::Fail(message) = preflight(false, None, true) else {
            panic!("expected Fail");
        };
        assert!(message.contains("Node.js"));
    }

    #[test]
    fn preflight_no_scope_no_tty_fails_naming_the_flags() {
        let Preflight::Fail(message) = preflight(true, None, false) else {
            panic!("expected Fail");
        };
        assert!(message.contains("--global") && message.contains("--local"));
    }

    #[test]
    fn preflight_no_scope_with_tty_asks() {
        assert_eq!(
            preflight(true, None, true),
            Preflight::Proceed { scope: None }
        );
    }

    #[test]
    fn preflight_scope_flag_proceeds_without_asking() {
        assert_eq!(
            preflight(true, Some(SkillsScope::Local), false),
            Preflight::Proceed {
                scope: Some(SkillsScope::Local)
            }
        );
    }
}
