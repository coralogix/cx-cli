use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::Command;
use clap_complete::aot::Shell;
use clap_complete::env::{Bash, EnvCompleter, Fish, Powershell, Zsh};

use crate::config;

// ── Default install paths ─────────────────────────────────────────────────────

pub fn default_install_path(shell: Shell) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = match shell {
        Shell::Zsh => home.join(".zfunc").join("_cx"),
        Shell::Bash => home
            .join(".local")
            .join("share")
            .join("bash-completion")
            .join("completions")
            .join("cx"),
        Shell::Fish => home
            .join(".config")
            .join("fish")
            .join("completions")
            .join("cx.fish"),
        // PowerShell has no single canonical user path - require --path.
        _ => return None,
    };
    Some(path)
}

/// The shells cx currently tracks an installed completion for. Used by the
/// guided `cx init` flow to stay idempotent: once a shell is selected (via the
/// picker or `--install-completions`), init skips the actual install when that
/// shell is already in this list, so re-running `cx init` never rewrites an
/// existing install. `cx completions install`/`refresh` force a rewrite.
pub fn installed_shells() -> Vec<Shell> {
    config::managed_completions()
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| entry.shell.parse::<Shell>().ok())
        .collect()
}

// ── Post-install setup notes ──────────────────────────────────────────────────

fn setup_note(shell: Shell, path: &Path) -> Option<String> {
    match shell {
        Shell::Zsh => Some(format!(
            "Make sure {} is in your $fpath. Add to ~/.zshrc if it isn't:\n  fpath=({} $fpath)\n  autoload -Uz compinit && compinit",
            path.parent()?.display(),
            path.parent()?.display()
        )),
        _ => None,
    }
}

// ── Registration script generation (dynamic env-based) ────────────────────────

/// Write the dynamic env-completion registration stub for the given shell.
///
/// Unlike `clap_complete::aot::generate`, this emits a small bootstrap that
/// calls back into `cx` at completion time - which means runtime completers
/// like `ArgValueCompleter` (used for `--profile=`) work correctly.
fn write_registration_for_shell(shell: Shell, buf: &mut dyn std::io::Write) -> Result<()> {
    // The shell-specific completer adapters all share the same registration
    // contract (see clap_complete::env::EnvCompleter). We pick the right
    // adapter based on our enum and call write_registration on it.
    fn write<C: EnvCompleter>(c: &C, buf: &mut dyn std::io::Write) -> Result<()> {
        c.write_registration("COMPLETE", "cx", "cx", "cx", buf)
            .map_err(|e| anyhow!("Failed to write completion registration: {e}"))
    }

    match shell {
        Shell::Bash => write(&Bash, buf),
        Shell::Zsh => write(&Zsh, buf),
        Shell::Fish => write(&Fish, buf),
        Shell::PowerShell => write(&Powershell, buf),
        other => Err(anyhow!(
            "Shell '{other}' is not supported by cx completions"
        )),
    }
}

// ── Atomic file write ─────────────────────────────────────────────────────────

/// Write `contents` to `path` atomically: stage in a sibling temp file and
/// rename into place. On any failure, the temp file is removed and any
/// existing file at `path` is left untouched, so we never leave half-written
/// completion scripts on disk.
fn write_atomically(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    let mut tmp_name = path
        .file_name()
        .ok_or_else(|| anyhow!("Invalid completion path: {}", path.display()))?
        .to_os_string();
    tmp_name.push(".cx-tmp");
    let tmp_path = path.with_file_name(tmp_name);

    if let Err(e) = std::fs::write(&tmp_path, contents) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(anyhow::Error::new(e).context(format!(
            "Failed to write completion script to {}",
            path.display()
        )));
    }

    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(anyhow::Error::new(e).context(format!(
            "Failed to install completion script at {}",
            path.display()
        )));
    }

    Ok(())
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Print a completion script for the given shell to stdout.
pub fn run_generate(shell: Shell, _clap_cmd: &mut Command) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    write_registration_for_shell(shell, &mut stdout)?;
    stdout.flush().ok();
    Ok(())
}

/// Install a completion script to a default (or specified) path and register it.
pub fn run_install(shell: Shell, path_override: Option<PathBuf>) -> Result<()> {
    let path = path_override
        .or_else(|| default_install_path(shell))
        .with_context(|| {
            format!(
                "No default install path for {shell}. \
                 Specify one with --path, e.g. `cx completions install {shell} --path /path/to/file`."
            )
        })?;

    let mut buf = Vec::new();
    write_registration_for_shell(shell, &mut buf)?;
    write_atomically(&path, &buf)?;

    // Track the install so `cx completions refresh` can update it later.
    // If tracking fails, roll back the file we just wrote so we don't leave
    // an untracked completion script on disk that we can't manage.
    if let Err(e) = config::upsert_managed_completion(&shell.to_string(), path.clone()) {
        let _ = std::fs::remove_file(&path);
        return Err(e).context("Failed to record managed completion in cx config");
    }

    eprintln!("Installed {} completions to {}", shell, path.display());

    if let Some(note) = setup_note(shell, &path) {
        eprintln!("\nNote: {note}");
    }

    Ok(())
}

/// Regenerate all completion scripts previously installed by `cx completions install`.
///
/// With dynamic env-based completions, the on-disk stub rarely needs updating
/// (it always delegates back to the current `cx` binary). Refreshing is still
/// useful after upgrading `cx` in case the registration protocol changed.
///
/// Each entry is refreshed atomically (temp file + rename), so a failure on
/// one entry leaves the existing file untouched and never produces a partial
/// write. We continue past per-entry failures so one bad path doesn't block
/// refreshing the others, but exit with an error summary if any failed.
pub fn run_refresh(_clap_cmd_factory: impl Fn() -> Command) -> Result<()> {
    let managed = config::managed_completions()?;

    if managed.is_empty() {
        eprintln!(
            "No managed completions found. \
             Run `cx completions install <shell>` first."
        );
        return Ok(());
    }

    let mut failed = 0usize;
    for entry in &managed {
        let result = (|| -> Result<()> {
            let shell: Shell = entry.shell.parse().map_err(|_| {
                anyhow!(
                    "Unrecognised shell '{}' in managed completions",
                    entry.shell
                )
            })?;

            let mut buf = Vec::new();
            write_registration_for_shell(shell, &mut buf)?;
            write_atomically(&entry.path, &buf)?;
            Ok(())
        })();

        match result {
            Ok(()) => eprintln!(
                "Refreshed {} completions at {}",
                entry.shell,
                entry.path.display()
            ),
            Err(e) => {
                failed += 1;
                eprintln!(
                    "Failed to refresh {} completions at {}: {e:#}",
                    entry.shell,
                    entry.path.display()
                );
            }
        }
    }

    if failed > 0 {
        anyhow::bail!(
            "{} of {} completions failed to refresh",
            failed,
            managed.len()
        );
    }

    Ok(())
}
