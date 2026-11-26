//! Skills install step: shells out to the vercel-labs `skills` npx installer
//! to install the cx agent skills bundle (`coralogix/cx-cli/skills`).
//!
//! Owns no skill/agent logic of its own beyond driving the one scope
//! question (global vs local) and passing flags through to the installer.
//! Installs are pinned to this version's release tag. `run_auto_refresh` is the
//! hidden background refresh that the update check starts after an upgrade.

use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
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

/// The vercel-labs installer package, pinned so a new installer release never
/// runs on users' machines untested, now that cx also runs it on its own
/// after an upgrade. Bump it on purpose, after trying the new version.
pub const SKILLS_PACKAGE: &str = "skills@1.8.1";

/// The running cx version. Installs are pinned to its release tag.
const CX_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The installer source pinned to the release tag of `version`
/// (`coralogix/cx-cli/skills#v0.1.31`), so the skills match the binary instead
/// of the default branch. A pre-release version (`0.1.32-dev`) has no tag, so
/// it gets the unpinned source.
fn pinned_source(version: &str) -> String {
    if version.contains('-') {
        SKILLS_SOURCE.to_string()
    } else {
        format!("{SKILLS_SOURCE}#v{version}")
    }
}

/// The command a user can run by hand when cx fails the install.
fn manual_install_cmd() -> String {
    format!("npx {SKILLS_PACKAGE} add {}", pinned_source(CX_VERSION))
}

/// The `cx skills install` command to repeat an install, carrying the scope
/// the user already chose (`--global`/`--local`) so the retry works in a
/// shell without a terminal, where the scope question cannot be asked. With
/// no scope yet, the plain command, which asks interactively.
pub fn install_command(scope: Option<SkillsScope>) -> String {
    match scope {
        Some(SkillsScope::Global) => "cx skills install --global".to_string(),
        Some(SkillsScope::Local) => "cx skills install --local".to_string(),
        None => "cx skills install".to_string(),
    }
}

/// Where to browse the installed skills and read their contents. Shown in the
/// compact summary as a pointer for manually reviewing what was installed;
/// the installer's own verbose output (including its security-risk table) is
/// suppressed in the non-interactive run, and this page does not replace that
/// table — use `cx skills install --interactive` to see the installer's full
/// output.
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
    /// When true (init), an already-installed detection skips the install
    /// (idempotent onboarding). When false (`cx skills install`), it
    /// reinstalls to update.
    pub skip_if_installed: bool,
}

/// What [`run_install`] did, so callers (init) can message accordingly
/// without probing the installer themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// The installer ran (fresh install or reinstall-to-update).
    Installed,
    /// Skipped: cx skills already installed in the target scope. Only
    /// returned with [`InstallOptions::skip_if_installed`].
    AlreadyInstalled,
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
        return Preflight::Fail(format!(
            "Node.js/npx is required to install the cx agent skills.\n\
             Coding agents use cx much better with these skills.\n\
             Install Node.js (https://nodejs.org), then run `{}`.",
            install_command(scope)
        ));
    }
    if scope.is_none() && !stdin_is_terminal {
        return Preflight::Fail(
            "no skills install scope - pass --global or --local \
             (--global-skills/--local-skills on `cx init`)"
                .into(),
        );
    }
    Preflight::Proceed { scope }
}

/// Install with at most one question (scope), then a fully
/// non-interactive `npx skills add` run.
pub fn run_install(opts: InstallOptions) -> Result<InstallOutcome> {
    let scope = match preflight(npx_available(), opts.scope, std::io::stdin().is_terminal()) {
        Preflight::Fail(message) => bail!(message),
        Preflight::Proceed { scope } => match scope {
            Some(scope) => scope,
            None => ask_scope()?,
        },
    };

    // The single already-installed probe (npx spawns are slow — seconds on a
    // cold first run — so callers must not duplicate it). Detection is scoped
    // to the scope we're about to install into, so skills present in the
    // other scope don't muddy the result.
    let installed = installed_cx_skills(scope);
    if !installed.is_empty() {
        if opts.skip_if_installed {
            return Ok(InstallOutcome::AlreadyInstalled);
        }
        println!(
            "cx agent skills already installed ({}) - reinstalling to update.",
            installed_summary(&installed)
        );
    }

    // The `-y` installer runs fully non-interactively, so it needs no terminal
    // and its verbose TUI (banner, per-skill table, risk matrix) is pure noise.
    // Capture it and replace it with a one-line summary; only surface the raw
    // output on failure, where the error detail matters. The user-driven
    // interactive walk (`run_advanced_install`) keeps full passthrough.
    println!("Installing cx agent skills…");

    let output = run_npx_install(scope, &opts.agents, Run::Foreground)?;
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
             Retry with `{}`, or run the installer directly: {}",
            output.status,
            install_command(Some(scope)),
            manual_install_cmd()
        );
    }

    print_install_summary(scope);
    // Only a global install is recorded: local skills belong to one project,
    // and the state file is shared by all of them.
    if scope == SkillsScope::Global {
        crate::version_cache::record_global_skills_install(&opts.agents);
    }
    Ok(InstallOutcome::Installed)
}

/// How the installer runs: attached to the user's terminal, or inside the
/// detached `cx skills auto-refresh`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Run {
    Foreground,
    Background,
}

/// How long the background refresh lets the installer run before it stops it.
/// Shorter than [`RefreshLock::STALE_AFTER`], so a stopped run never overlaps
/// the next one.
const BACKGROUND_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Run the non-interactive install from this version's release tag. If the
/// tag does not exist yet, retry once from the default branch, so a missing
/// tag never blocks the install. Any other failure (network, auth, installer)
/// is returned as is: falling back then would install unpinned skills that
/// may not match this binary.
fn run_npx_install(
    scope: SkillsScope,
    agents: &[String],
    run: Run,
) -> Result<std::process::Output> {
    let install = |source: &str| {
        let mut cmd = Command::new(npx_program());
        cmd.args(build_install_args(source, scope, agents));
        match run {
            Run::Foreground => cmd.output().context("failed to run npx"),
            Run::Background => output_in_background(cmd, BACKGROUND_TIMEOUT),
        }
    };
    let pinned = pinned_source(CX_VERSION);
    let output = install(&pinned)?;
    if output.status.success() || pinned == SKILLS_SOURCE || !is_missing_ref(&output) {
        return Ok(output);
    }
    install(SKILLS_SOURCE)
}

/// Run `cmd` with its output captured, for the background refresh:
/// - with no console window on Windows (`npx.cmd` runs through `cmd.exe`, and
///   the detached refresh has no console to share, so Windows would open one);
/// - in its own process group on Unix, so a stop also ends the node processes
///   that `npx` starts;
/// - stopped after `timeout`, so an installer that hangs (a proxy, a registry
///   login prompt) does not stay forever.
fn output_in_background(mut cmd: Command, timeout: Duration) -> Result<std::process::Output> {
    use std::io::Read;
    use std::process::Stdio;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to run npx")?;
    // Read both pipes while waiting, so a full pipe never blocks the child.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buf);
            }
            buf
        })
    };
    let stdout = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let stderr = drain(child.stderr.take().map(|p| Box::new(p) as _));
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            stop_process_tree(&mut child);
            bail!(
                "skills installer did not finish within {}s",
                timeout.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    Ok(std::process::Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// Stop `child` and everything it started.
fn stop_process_tree(child: &mut std::process::Child) {
    let pid = child.id().to_string();
    #[cfg(unix)]
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .status();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// True when the installer failed because the git ref (the release tag) does
/// not exist. Same patterns as the installer's own `isMissingRefError`; git's
/// message reaches either stream inside the installer's
/// `SKILLS_GIT_CLONE_FAILED` diagnostic.
fn is_missing_ref(output: &std::process::Output) -> bool {
    [&output.stdout, &output.stderr].iter().any(|captured| {
        let text = String::from_utf8_lossy(captured).to_lowercase();
        (text.contains("remote branch") && text.contains("not found in upstream origin"))
            || text.contains("couldn't find remote ref")
    })
}

/// The hidden `cx skills auto-refresh` command, started in the background by
/// the update check after a cx upgrade (see `update_check`). Repeats the
/// recorded global install (same agents) with this version's skills, with no
/// output. When no install was recorded (skills from an older cx or plain
/// `npx`), there are no recorded agents, so the installer auto-detects them,
/// as `cx skills install --global` does. Exits quietly on any problem: the
/// update notice is the fallback.
pub fn run_auto_refresh() -> Result<()> {
    let Some(_lock) = RefreshLock::acquire() else {
        return Ok(());
    };
    let agents = crate::version_cache::VersionCheckCache::load().skills_agents;
    let output = run_npx_install(SkillsScope::Global, &agents, Run::Background)?;
    if output.status.success() {
        crate::version_cache::record_global_skills_install(&agents);
    }
    Ok(())
}

/// A lock file (`~/.cx/skills-refresh.lock`) so two cx commands that run at
/// the same time do not start two installs. A lock older than
/// [`RefreshLock::STALE_AFTER`] is left over from a killed run and is replaced.
struct RefreshLock(std::path::PathBuf);

impl RefreshLock {
    const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(15 * 60);

    fn acquire() -> Option<Self> {
        // `~/.cx` may not exist yet (skills installed with plain `npx`, and
        // the parent's state write may not have landed), and `create_new`
        // needs the directory.
        let dir = crate::config::config_dir().ok()?;
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join("skills-refresh.lock");
        let stale = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > Self::STALE_AFTER);
        if stale {
            let _ = std::fs::remove_file(&path);
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .ok()?;
        Some(Self(path))
    }
}

impl Drop for RefreshLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// True when an `npx` executable is on `PATH`. A plain directory scan, so it
/// is cheap enough for the update check, unlike spawning `npx --version`.
pub fn npx_on_path() -> bool {
    std::env::var_os("PATH").is_some_and(|path| npx_in(&path))
}

fn npx_in(path: &std::ffi::OsStr) -> bool {
    std::env::split_paths(path).any(|dir| dir.join(npx_program()).is_file())
}

/// The compact success line shown after a non-interactive install, in place of
/// the installer's verbose output. Points at the skill listing so the user can
/// review what was installed, since the installer's verbose output was hidden.
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
             Browse them at: {SKILLS_REVIEW_URL}"
        );
    } else {
        // Installer succeeded but detection came up empty (e.g. an older
        // installer without `--json`); still confirm and point to review.
        println!(
            "✓ cx agent skills installed ({scope_label}). \
             Browse them at: {SKILLS_REVIEW_URL}"
        );
    }
}

/// Advanced install: run the raw installer with no flags and let the user
/// walk its full interactive flow (skill/agent selection, scope, method).
pub fn run_advanced_install() -> Result<()> {
    let source = pinned_source(CX_VERSION);
    println!("Running the skills installer interactively: npx {SKILLS_PACKAGE} add {source}");
    let status = Command::new(npx_program())
        .args([SKILLS_PACKAGE, "add", &source])
        .status()
        .context(
            "failed to run npx - Node.js is required for the skills install \
             (https://nodejs.org)",
        )?;
    if !status.success() {
        bail!("skills installer exited with {status}");
    }
    // Not recorded: the installer exits 0 when the user cancels, and the user
    // may pick local scope or only some skills, so success says nothing about
    // which skills are now current.
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

/// cx skills already installed in `scope`, according to the installer itself
/// (`npx skills ls [-g] --json`). A skill counts as ours when its lock-file
/// source is [`INSTALLED_SOURCE`]. Any failure (old installer without `--json`,
/// unparseable output) counts as nothing installed, so the install proceeds.
///
/// Copy installs (`--copy`) drop the lock-file source and so are not detected;
/// the installer's default is symlink, which preserves it.
fn installed_cx_skills(scope: SkillsScope) -> Vec<String> {
    let mut cmd = Command::new(npx_program());
    cmd.args(["-y", SKILLS_PACKAGE, "ls"]);
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

/// cx skills listed in the installer's global lock file.
#[derive(Debug, PartialEq, Eq)]
pub struct LockedCxSkills {
    pub skills: Vec<LockedCxSkill>,
}

/// One cx skill in the installer's global lock file.
#[derive(Debug, PartialEq, Eq)]
pub struct LockedCxSkill {
    /// The git ref it was installed from: a release tag such as `v0.1.31`
    /// for pinned installs, `None` for the default branch.
    pub git_ref: Option<String>,
    /// `updatedAt`, else `installedAt`. `None` when neither is readable.
    pub updated: Option<DateTime<Utc>>,
}

/// The cx skills in the installer's global lock file, or `None` when it lists
/// none. A plain file read, so it is cheap enough to run after every command,
/// unlike `npx skills ls`. It also sees skills refreshed with `npx skills add`
/// directly, which cx does not record.
pub fn global_lock_cx_skills() -> Option<LockedCxSkills> {
    let xdg_state_home = std::env::var_os("XDG_STATE_HOME").map(std::path::PathBuf::from);
    let path = global_lock_path(xdg_state_home, dirs::home_dir())?;
    let contents = std::fs::read_to_string(path).ok()?;
    lock_cx_skills(&contents)
}

/// Same lookup as the installer's `getSkillLockPath`: `$XDG_STATE_HOME` when
/// set (and not empty), else `~/.agents`.
fn global_lock_path(
    xdg_state_home: Option<std::path::PathBuf>,
    home: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    match xdg_state_home {
        Some(xdg) if !xdg.as_os_str().is_empty() => {
            Some(xdg.join("skills").join(".skill-lock.json"))
        }
        _ => home.map(|h| h.join(".agents").join(".skill-lock.json")),
    }
}

/// The lock file maps skill names to entries that carry a `source`, an
/// optional `ref`, and ISO-8601 `installedAt` / `updatedAt` times.
fn lock_cx_skills(contents: &str) -> Option<LockedCxSkills> {
    #[derive(Deserialize)]
    struct Lock {
        #[serde(default)]
        skills: std::collections::BTreeMap<String, LockEntry>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct LockEntry {
        source: Option<String>,
        #[serde(rename = "ref")]
        git_ref: Option<String>,
        installed_at: Option<String>,
        updated_at: Option<String>,
    }
    let lock = serde_json::from_str::<Lock>(contents).ok()?;
    let skills: Vec<LockedCxSkill> = lock
        .skills
        .into_values()
        .filter(|entry| entry.source.as_deref() == Some(INSTALLED_SOURCE))
        .map(|entry| LockedCxSkill {
            updated: entry
                .updated_at
                .as_deref()
                .or(entry.installed_at.as_deref())
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&Utc)),
            git_ref: entry.git_ref,
        })
        .collect();
    (!skills.is_empty()).then_some(LockedCxSkills { skills })
}

/// Arguments for the fully non-interactive install.
///
/// The leading `-y` is npx's own auto-approve (first-run package download
/// prompt); the trailing `-y` is the installer's skip-all-confirmations.
fn build_install_args(source: &str, scope: SkillsScope, agents: &[String]) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-y".into(),
        SKILLS_PACKAGE.into(),
        "add".into(),
        source.into(),
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
        let args = build_install_args(SKILLS_SOURCE, SkillsScope::Local, &[]);
        assert_eq!(
            args,
            [
                "-y",
                SKILLS_PACKAGE,
                "add",
                SKILLS_SOURCE,
                "--skill",
                "*",
                "-y"
            ]
        );
    }

    #[test]
    fn install_args_global_scope() {
        let args = build_install_args(SKILLS_SOURCE, SkillsScope::Global, &[]);
        assert_eq!(
            args,
            [
                "-y",
                SKILLS_PACKAGE,
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
            SKILLS_SOURCE,
            SkillsScope::Global,
            &["claude-code".to_string(), "cursor".to_string()],
        );
        assert_eq!(
            args,
            [
                "-y",
                SKILLS_PACKAGE,
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
    fn global_lock_path_prefers_xdg_state_home() {
        use std::path::PathBuf;
        let home = Some(PathBuf::from("/home/u"));
        assert_eq!(
            global_lock_path(Some(PathBuf::from("/state")), home.clone()),
            Some(PathBuf::from("/state/skills/.skill-lock.json"))
        );
        assert_eq!(
            global_lock_path(Some(PathBuf::new()), home.clone()),
            Some(PathBuf::from("/home/u/.agents/.skill-lock.json"))
        );
        assert_eq!(
            global_lock_path(None, home),
            Some(PathBuf::from("/home/u/.agents/.skill-lock.json"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn background_run_stops_a_hung_installer_and_its_children() {
        let pid_file = std::env::temp_dir().join(format!("cx-hung-{}", std::process::id()));
        let mut cmd = Command::new("sh");
        // A child that would outlive its parent, like the node process npx starts.
        cmd.args([
            "-c",
            &format!("sleep 30 & echo $! > {}; wait", pid_file.display()),
        ]);
        let started = std::time::Instant::now();
        let err = output_in_background(cmd, Duration::from_millis(500)).unwrap_err();
        assert!(err.to_string().contains("did not finish"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10));
        let grandchild = std::fs::read_to_string(&pid_file).unwrap();
        let _ = std::fs::remove_file(&pid_file);
        // The group is sent SIGKILL, but the orphaned grandchild stays visible
        // to `kill -0` until init reaps it, which can take a moment on a busy
        // CI runner. Poll briefly instead of checking once.
        let gone_by = std::time::Instant::now() + Duration::from_secs(5);
        let alive = loop {
            let alive = Command::new("kill")
                .args(["-0", grandchild.trim()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success();
            if !alive || std::time::Instant::now() >= gone_by {
                break alive;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(!alive, "the installer's child must be stopped too");
    }

    #[cfg(unix)]
    #[test]
    fn background_run_returns_the_installer_output() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo out; echo err >&2; exit 3"]);
        let output = output_in_background(cmd, Duration::from_secs(10)).unwrap();
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(output.stdout, b"out\n");
        assert_eq!(output.stderr, b"err\n");
    }

    #[cfg(unix)]
    fn output_with(code: i32, stdout: &str, stderr: &str) -> std::process::Output {
        use std::os::unix::process::ExitStatusExt;
        std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn missing_release_tag_is_detected() {
        // Captured from `npx skills add coralogix/cx-cli/skills#v0.0.999`.
        let stdout = "■  Failed to clone repository\n\
            │  [SKILLS_GIT_CLONE_FAILED] Failed to clone https://github.com/coralogix/cx-cli.git: \
            Cloning into '/tmp/skills-x'...\n\
            │  fatal: Remote branch v0.0.999 not found in upstream origin\n";
        assert!(is_missing_ref(&output_with(1, stdout, "")));
        assert!(is_missing_ref(&output_with(
            1,
            "",
            "fatal: couldn't find remote ref refs/tags/v0.0.999"
        )));
    }

    #[cfg(unix)]
    #[test]
    fn other_install_failures_are_not_a_missing_tag() {
        for text in [
            "[SKILLS_GIT_CLONE_TIMEOUT] Clone timed out after 60s",
            "[SKILLS_GIT_AUTH_FAILED] Authentication failed for https://github.com/coralogix/cx-cli.git",
            "fatal: unable to access 'https://github.com/coralogix/cx-cli.git/': Could not resolve host: github.com",
        ] {
            assert!(!is_missing_ref(&output_with(1, text, "")), "{text}");
            assert!(!is_missing_ref(&output_with(1, "", text)), "{text}");
        }
    }

    fn time(t: &str) -> Option<DateTime<Utc>> {
        Some(t.parse().unwrap())
    }

    #[test]
    fn lock_lists_each_cx_skill_with_ref_and_time() {
        let lock = r#"{"version":3,"skills":{
            "other":{"source":"someone/else","updatedAt":"2020-01-01T00:00:00.000Z"},
            "cx-alerts":{"source":"coralogix/cx-cli","ref":"v0.1.31",
                "installedAt":"2026-09-01T10:00:00.000Z","updatedAt":"2026-10-05T08:00:00.000Z"},
            "cx-cli":{"source":"coralogix/cx-cli","installedAt":"2026-10-01T09:30:00.000Z"},
            "cx-slos":{"source":"coralogix/cx-cli","updatedAt":"not a time"}
        }}"#;
        assert_eq!(
            lock_cx_skills(lock),
            Some(LockedCxSkills {
                skills: vec![
                    LockedCxSkill {
                        git_ref: Some("v0.1.31".into()),
                        updated: time("2026-10-05T08:00:00Z"),
                    },
                    LockedCxSkill {
                        git_ref: None,
                        updated: time("2026-10-01T09:30:00Z"),
                    },
                    LockedCxSkill {
                        git_ref: None,
                        updated: None,
                    },
                ]
            })
        );
    }

    #[test]
    fn lock_without_cx_skills_is_not_detected() {
        assert_eq!(
            lock_cx_skills(r#"{"version":3,"skills":{"other":{"source":"someone/else"}}}"#),
            None
        );
        assert_eq!(lock_cx_skills(r#"{"version":3}"#), None);
        assert_eq!(lock_cx_skills("not json"), None);
    }

    #[test]
    fn release_versions_pin_to_their_tag() {
        assert_eq!(pinned_source("0.1.31"), "coralogix/cx-cli/skills#v0.1.31");
        // Pre-release builds have no tag: use the default branch.
        assert_eq!(pinned_source("0.1.32-dev"), SKILLS_SOURCE);
        assert_eq!(
            build_install_args(&pinned_source("0.1.31"), SkillsScope::Global, &[])[3],
            "coralogix/cx-cli/skills#v0.1.31"
        );
    }

    #[test]
    fn npx_on_path_scans_path_dirs() {
        let dir = std::env::temp_dir().join(format!("cx-npx-scan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = std::env::join_paths([&dir]).unwrap();
        assert!(!npx_in(&path));
        std::fs::write(dir.join(npx_program()), "").unwrap();
        assert!(npx_in(&path));
        std::fs::remove_dir_all(&dir).unwrap();
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
        assert!(message.contains("`cx skills install`"));
        assert!(!message.contains("skip"));
    }

    /// The retry hint keeps the scope the user already chose, so a coding
    /// agent (no terminal, so no scope question) can follow it as written.
    #[test]
    fn preflight_no_npx_hint_keeps_the_chosen_scope() {
        for (scope, expected) in [
            (SkillsScope::Global, "`cx skills install --global`"),
            (SkillsScope::Local, "`cx skills install --local`"),
        ] {
            let Preflight::Fail(message) = preflight(false, Some(scope), false) else {
                panic!("expected Fail");
            };
            assert!(
                message.contains(expected),
                "expected {expected} in: {message}"
            );
        }
    }

    #[test]
    fn install_command_names_the_scope_flag() {
        assert_eq!(
            install_command(Some(SkillsScope::Global)),
            "cx skills install --global"
        );
        assert_eq!(
            install_command(Some(SkillsScope::Local)),
            "cx skills install --local"
        );
        assert_eq!(install_command(None), "cx skills install");
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
