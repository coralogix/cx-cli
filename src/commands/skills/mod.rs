//! Skills install step: shells out to the vercel-labs `skills` npx installer
//! to install the cx agent skills bundle (`coralogix/cx-cli`).
//!
//! `cx init` (FORGE-658) invokes this step; it owns no skill/agent logic of
//! its own beyond driving the one express question (global vs local scope)
//! and passing flags through to the installer.

use std::io::IsTerminal;
use std::process::Command;

use anyhow::{bail, Context, Result};
use inquire::Select;

use crate::request_metadata::installed_coralogix_skills;

/// The skills source passed to the installer, always fixed.
pub const SKILLS_SOURCE: &str = "coralogix/cx-cli";

/// The command a user can run by hand when cx skips or fails the install.
const MANUAL_INSTALL_CMD: &str = "npx skills add coralogix/cx-cli";

/// Where the installer puts skills: the user's home (`~/`) or the project (`./`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillsScope {
    Global,
    Local,
}

/// How the skills step was requested. Decides how obstacles are handled:
/// an implied run (the `cx init` default) skips with guidance, an explicit
/// run (`--skills` / `cx skills install`) fails with an actionable error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallRequest {
    Implied,
    Explicit,
}

/// What the install step ended up doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    Installed,
    AlreadyInstalled,
    /// npx is unavailable; the manual command was printed instead.
    SkippedNoNpx,
    /// Non-interactive implied run with no `--global`/`--local`; skipped.
    SkippedNoScope,
}

pub struct InstallOptions {
    /// Scope from `--global`/`--local`; `None` asks interactively.
    pub scope: Option<SkillsScope>,
    /// Agents passed through to the installer's `-a` (overrides auto-detect).
    pub agents: Vec<String>,
    pub request: InstallRequest,
}

/// Express install: at most one question (scope), then a fully
/// non-interactive `npx skills add` run.
pub fn run_install(opts: InstallOptions) -> Result<InstallOutcome> {
    let explicit = opts.request == InstallRequest::Explicit;

    if !npx_available() {
        if explicit {
            bail!(
                "Node.js/npx is required to install the cx agent skills.\n\
                 Install Node.js (https://nodejs.org) and rerun, or skip the skills install."
            );
        }
        eprintln!(
            "warning: npx not found - skipping the agent skills install.\n\
             The CLI is fully usable without skills. To install them later, \
             install Node.js and run:\n  {MANUAL_INSTALL_CMD}"
        );
        return Ok(InstallOutcome::SkippedNoNpx);
    }

    let installed = installed_coralogix_skills();
    if !installed.is_empty() {
        let summary = installed_summary(&installed);
        if explicit {
            println!("cx agent skills already installed ({summary}) - reinstalling to update.");
        } else {
            println!(
                "cx agent skills already installed ({summary}) - skipping.\n\
                 To update them, run: {MANUAL_INSTALL_CMD}"
            );
            return Ok(InstallOutcome::AlreadyInstalled);
        }
    }

    let scope = match opts.scope {
        Some(scope) => scope,
        None => {
            if !std::io::stdin().is_terminal() {
                if explicit {
                    bail!("no skills install scope - pass --global or --local");
                }
                eprintln!(
                    "warning: no terminal to ask for the skills install scope - \
                     skipping the agent skills install.\n\
                     To install them later, run: {MANUAL_INSTALL_CMD}"
                );
                return Ok(InstallOutcome::SkippedNoScope);
            }
            ask_scope()?
        }
    };

    let args = build_install_args(scope, &opts.agents);
    println!("Installing cx agent skills: npx {}", args.join(" "));

    let status = Command::new(npx_program())
        .args(&args)
        .status()
        .context("failed to run npx")?;
    if !status.success() {
        bail!(
            "skills installer exited with {status}.\n\
             You can retry manually with: {MANUAL_INSTALL_CMD}"
        );
    }

    println!("cx agent skills installed.");
    Ok(InstallOutcome::Installed)
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

/// Arguments for the fully non-interactive express install.
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
    .with_starting_cursor(0)
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
    fn express_args_local_scope() {
        let args = build_install_args(SkillsScope::Local, &[]);
        assert_eq!(
            args,
            ["-y", "skills", "add", SKILLS_SOURCE, "--skill", "*", "-y"]
        );
    }

    #[test]
    fn express_args_global_scope() {
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
    fn express_args_pass_agents_through() {
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
}
