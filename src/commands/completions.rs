use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::Command;
use clap_complete::aot::Shell;
use clap_complete::env::{Bash, Elvish, EnvCompleter, Fish, Powershell, Zsh};

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
        Shell::Elvish => home.join(".elvish").join("lib").join("cx-completions.elv"),
        // PowerShell has no single canonical user path — require --path.
        _ => return None,
    };
    Some(path)
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
/// calls back into `cx` at completion time — which means runtime completers
/// like `ArgValueCompleter` (used for `--profile=`) work correctly.
fn write_registration_for(shell: Shell, buf: &mut dyn std::io::Write) -> Result<()> {
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
        Shell::Elvish => write(&Elvish, buf),
        Shell::PowerShell => write(&Powershell, buf),
        other => Err(anyhow!(
            "Shell '{other}' is not supported by dynamic completions"
        )),
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Print a completion script for the given shell to stdout.
pub fn run_generate(shell: Shell, _clap_cmd: &mut Command) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    write_registration_for(shell, &mut stdout)?;
    stdout.flush().ok();
    Ok(())
}

/// Install a completion script to a default (or specified) path and register it.
pub fn run_install(
    shell: Shell,
    path_override: Option<PathBuf>,
    _clap_cmd: &mut Command,
) -> Result<()> {
    let path = path_override
        .or_else(|| default_install_path(shell))
        .with_context(|| {
            format!(
                "No default install path for {shell}. \
                 Specify one with --path, e.g. `cx completions install {shell} --path /path/to/file`."
            )
        })?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    let mut buf = Vec::new();
    write_registration_for(shell, &mut buf)?;
    std::fs::write(&path, &buf)
        .with_context(|| format!("Failed to write completion script to {}", path.display()))?;

    eprintln!("Installed {} completions to {}", shell, path.display());

    if let Some(note) = setup_note(shell, &path) {
        eprintln!("\nNote: {note}");
    }

    config::upsert_managed_completion(&shell.to_string(), path)?;

    Ok(())
}

/// Regenerate all completion scripts previously installed by `cx completions install`.
///
/// With dynamic env-based completions, the on-disk stub rarely needs updating
/// (it always delegates back to the current `cx` binary). Refreshing is still
/// useful after upgrading `cx` in case the registration protocol changed.
pub fn run_refresh(_clap_cmd_factory: impl Fn() -> Command) -> Result<()> {
    let managed = config::managed_completions()?;

    if managed.is_empty() {
        eprintln!(
            "No managed completions found. \
             Run `cx completions install <shell>` first."
        );
        return Ok(());
    }

    for entry in &managed {
        let shell: Shell = entry.shell.parse().map_err(|_| {
            anyhow::anyhow!(
                "Unrecognised shell '{}' in managed completions",
                entry.shell
            )
        })?;

        let mut buf = Vec::new();
        write_registration_for(shell, &mut buf)?;

        if let Some(parent) = entry.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }

        std::fs::write(&entry.path, &buf).with_context(|| {
            format!(
                "Failed to write completion script to {}",
                entry.path.display()
            )
        })?;

        eprintln!(
            "Refreshed {} completions at {}",
            entry.shell,
            entry.path.display()
        );
    }

    Ok(())
}
