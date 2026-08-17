//! Skills install step: shells out to the vercel-labs `skills` npx installer
//! to install the cx agent skills bundle (`coralogix/cx-cli`).
//!
//! `cx init` (FORGE-658) invokes this step; it owns no skill/agent logic of
//! its own beyond driving the one express question (global vs local scope)
//! and passing flags through to the installer.

use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::process::Command;

use anyhow::{bail, Context, Result};
use inquire::Select;
use serde::Deserialize;

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

/// The decision run_install makes before touching the installer, kept pure
/// so every branch (including the implied ones only `cx init` reaches) is
/// unit-testable.
#[derive(Debug, PartialEq, Eq)]
enum InstallPlan {
    Fail(String),
    Skip {
        outcome: InstallOutcome,
        message: String,
    },
    Proceed {
        /// `None` means ask interactively.
        scope: Option<SkillsScope>,
        reinstall_note: Option<String>,
    },
}

fn plan_install(
    npx_available: bool,
    installed: &[String],
    scope: Option<SkillsScope>,
    stdin_is_terminal: bool,
    request: InstallRequest,
) -> InstallPlan {
    let explicit = request == InstallRequest::Explicit;

    if !npx_available {
        if explicit {
            return InstallPlan::Fail(
                "Node.js/npx is required to install the cx agent skills.\n\
                 Install Node.js (https://nodejs.org) and rerun, or skip the skills install."
                    .to_string(),
            );
        }
        return InstallPlan::Skip {
            outcome: InstallOutcome::SkippedNoNpx,
            message: format!(
                "warning: npx not found - skipping the agent skills install.\n\
                 The CLI is fully usable without skills. To install them later, \
                 install Node.js and run:\n  {MANUAL_INSTALL_CMD}"
            ),
        };
    }

    let reinstall_note = if installed.is_empty() {
        None
    } else {
        let summary = installed_summary(installed);
        if !explicit {
            return InstallPlan::Skip {
                outcome: InstallOutcome::AlreadyInstalled,
                message: format!(
                    "cx agent skills already installed ({summary}) - skipping.\n\
                     To update them, run: {MANUAL_INSTALL_CMD}"
                ),
            };
        }
        Some(format!(
            "cx agent skills already installed ({summary}) - reinstalling to update."
        ))
    };

    if scope.is_none() && !stdin_is_terminal {
        if explicit {
            return InstallPlan::Fail("no skills install scope - pass --global or --local".into());
        }
        return InstallPlan::Skip {
            outcome: InstallOutcome::SkippedNoScope,
            message: format!(
                "warning: no terminal to ask for the skills install scope - \
                 skipping the agent skills install.\n\
                 To install them later, run: {MANUAL_INSTALL_CMD}"
            ),
        };
    }

    InstallPlan::Proceed {
        scope,
        reinstall_note,
    }
}

/// Express install: at most one question (scope), then a fully
/// non-interactive `npx skills add` run.
pub fn run_install(opts: InstallOptions) -> Result<InstallOutcome> {
    let npx_available = npx_available();
    let installed = if npx_available {
        installed_cx_skills()
    } else {
        Vec::new()
    };
    let plan = plan_install(
        npx_available,
        &installed,
        opts.scope,
        std::io::stdin().is_terminal(),
        opts.request,
    );

    let (scope, reinstall_note) = match plan {
        InstallPlan::Fail(message) => bail!(message),
        InstallPlan::Skip { outcome, message } => {
            if outcome == InstallOutcome::AlreadyInstalled {
                println!("{message}");
            } else {
                eprintln!("{message}");
            }
            return Ok(outcome);
        }
        InstallPlan::Proceed {
            scope,
            reinstall_note,
        } => (scope, reinstall_note),
    };

    if let Some(note) = reinstall_note {
        println!("{note}");
    }
    let scope = match scope {
        Some(scope) => scope,
        None => ask_scope()?,
    };

    let args = build_install_args(scope, &opts.agents);
    println!("Installing cx agent skills: npx {}", display_args(&args));

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

/// One entry of `skills ls --json` output; extra fields are ignored.
#[derive(Deserialize)]
struct LsSkill {
    name: String,
    #[serde(default)]
    source: Option<String>,
}

/// cx skills already installed, according to the installer itself
/// (`npx skills ls --json`, project and global scope). A skill counts as
/// ours when its lock-file source is `coralogix/cx-cli` or its name carries
/// the cx prefix. Any failure (old installer without `--json`, unparseable
/// output) counts as nothing installed, so the install proceeds.
fn installed_cx_skills() -> Vec<String> {
    let mut names = BTreeSet::new();
    for global in [false, true] {
        let mut cmd = Command::new(npx_program());
        cmd.args(["-y", "skills", "ls"]);
        if global {
            cmd.arg("-g");
        }
        cmd.arg("--json");
        let Ok(output) = cmd.output() else { continue };
        if !output.status.success() {
            continue;
        }
        let Ok(skills) = serde_json::from_slice::<Vec<LsSkill>>(&output.stdout) else {
            continue;
        };
        for skill in skills {
            if skill.source.as_deref() == Some(SKILLS_SOURCE)
                || skill.name.starts_with("cx-")
                || skill.name.starts_with("coralogix-")
            {
                names.insert(skill.name);
            }
        }
    }
    names.into_iter().collect()
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

/// Shell-quoted rendering of the args, safe to copy back into a shell
/// (`*` would otherwise glob-expand).
fn display_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            let safe = !arg.is_empty()
                && arg
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./:@=".contains(c));
            if safe {
                arg.clone()
            } else {
                format!("'{arg}'")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    fn displayed_command_is_shell_safe() {
        let args = build_install_args(SkillsScope::Global, &[]);
        assert_eq!(
            display_args(&args),
            "-y skills add coralogix/cx-cli --skill '*' -y -g"
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
    fn ls_json_parses_and_ignores_extra_fields() {
        let json = r#"[
            {"name": "cx-alerts", "path": "/p", "scope": "global",
             "agents": ["Claude Code"], "source": "coralogix/cx-cli",
             "sourceUrl": null, "sourceType": "github"},
            {"name": "other-skill", "source": "someone/else"},
            {"name": "local-skill", "source": null}
        ]"#;
        let skills: Vec<LsSkill> = serde_json::from_str(json).unwrap();
        let ours: Vec<&str> = skills
            .iter()
            .filter(|s| s.source.as_deref() == Some(SKILLS_SOURCE) || s.name.starts_with("cx-"))
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(ours, ["cx-alerts"]);
    }

    // ── plan_install: every branch, including the implied ones `cx init` uses ──

    const NO_SKILLS: &[String] = &[];

    fn one_skill() -> Vec<String> {
        vec!["cx-alerts".to_string()]
    }

    #[test]
    fn plan_no_npx_explicit_fails_actionably() {
        let plan = plan_install(false, NO_SKILLS, None, true, InstallRequest::Explicit);
        let InstallPlan::Fail(message) = plan else {
            panic!("expected Fail, got {plan:?}");
        };
        assert!(message.contains("Node.js"));
    }

    #[test]
    fn plan_no_npx_implied_skips_with_manual_command() {
        let plan = plan_install(false, NO_SKILLS, None, true, InstallRequest::Implied);
        let InstallPlan::Skip { outcome, message } = plan else {
            panic!("expected Skip, got {plan:?}");
        };
        assert_eq!(outcome, InstallOutcome::SkippedNoNpx);
        assert!(message.contains(MANUAL_INSTALL_CMD));
    }

    #[test]
    fn plan_already_installed_implied_skips_with_update_hint() {
        let plan = plan_install(
            true,
            &one_skill(),
            Some(SkillsScope::Global),
            true,
            InstallRequest::Implied,
        );
        let InstallPlan::Skip { outcome, message } = plan else {
            panic!("expected Skip, got {plan:?}");
        };
        assert_eq!(outcome, InstallOutcome::AlreadyInstalled);
        assert!(message.contains("cx-alerts") && message.contains(MANUAL_INSTALL_CMD));
    }

    #[test]
    fn plan_already_installed_explicit_proceeds_with_note() {
        let plan = plan_install(
            true,
            &one_skill(),
            Some(SkillsScope::Global),
            true,
            InstallRequest::Explicit,
        );
        assert_eq!(
            plan,
            InstallPlan::Proceed {
                scope: Some(SkillsScope::Global),
                reinstall_note: Some(
                    "cx agent skills already installed (1 skills: cx-alerts) \
                     - reinstalling to update."
                        .to_string()
                ),
            }
        );
    }

    #[test]
    fn plan_no_scope_no_tty_explicit_fails_naming_the_flags() {
        let plan = plan_install(true, NO_SKILLS, None, false, InstallRequest::Explicit);
        let InstallPlan::Fail(message) = plan else {
            panic!("expected Fail, got {plan:?}");
        };
        assert!(message.contains("--global") && message.contains("--local"));
    }

    #[test]
    fn plan_no_scope_no_tty_implied_skips() {
        let plan = plan_install(true, NO_SKILLS, None, false, InstallRequest::Implied);
        let InstallPlan::Skip { outcome, message } = plan else {
            panic!("expected Skip, got {plan:?}");
        };
        assert_eq!(outcome, InstallOutcome::SkippedNoScope);
        assert!(message.contains(MANUAL_INSTALL_CMD));
    }

    #[test]
    fn plan_no_scope_with_tty_asks() {
        let plan = plan_install(true, NO_SKILLS, None, true, InstallRequest::Implied);
        assert_eq!(
            plan,
            InstallPlan::Proceed {
                scope: None,
                reinstall_note: None,
            }
        );
    }

    #[test]
    fn plan_scope_flag_proceeds_without_asking() {
        let plan = plan_install(
            true,
            NO_SKILLS,
            Some(SkillsScope::Local),
            false,
            InstallRequest::Explicit,
        );
        assert_eq!(
            plan,
            InstallPlan::Proceed {
                scope: Some(SkillsScope::Local),
                reinstall_note: None,
            }
        );
    }
}
