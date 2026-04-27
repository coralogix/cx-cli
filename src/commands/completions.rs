use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Command;
use clap_complete::aot::{generate, Shell};

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

// ── Commands ──────────────────────────────────────────────────────────────────

/// Print a completion script for the given shell to stdout.
pub fn run_generate(shell: Shell, clap_cmd: &mut Command) {
    generate(shell, clap_cmd, "cx", &mut std::io::stdout());
}

/// Install a completion script to a default (or specified) path and register it.
pub fn run_install(
    shell: Shell,
    path_override: Option<PathBuf>,
    clap_cmd: &mut Command,
) -> Result<()> {
    let path = path_override
        .or_else(|| default_install_path(shell))
        .with_context(|| {
            format!(
                "No default install path for {shell}. \
                 Specify one with --path, e.g. `cx completions install {shell} --path /path/to/file`."
            )
        })?;

    // Create parent directories.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    // Generate and write to file.
    let mut buf = Vec::new();
    generate(shell, clap_cmd, "cx", &mut buf);
    std::fs::write(&path, &buf)
        .with_context(|| format!("Failed to write completion script to {}", path.display()))?;

    eprintln!("Installed {} completions to {}", shell, path.display());

    if let Some(note) = setup_note(shell, &path) {
        eprintln!("\nNote: {note}");
    }

    // Record so `cx completions refresh` can update the file later.
    config::upsert_managed_completion(&shell.to_string(), path)?;

    Ok(())
}

/// Regenerate all completion scripts previously installed by `cx completions install`.
pub fn run_refresh(clap_cmd_factory: impl Fn() -> Command) -> Result<()> {
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
        let mut cmd = clap_cmd_factory();
        generate(shell, &mut cmd, "cx", &mut buf);

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
