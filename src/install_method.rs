//! Detect how `cx` was installed so we can suggest the right upgrade command.

#[derive(Debug, PartialEq, Eq)]
pub enum InstallMethod {
    Brew,
    Curl,
    Unknown,
}

/// Detect install method from the running binary's path and a marker file.
///
/// Order of checks:
/// 1. Binary resolves into a Homebrew `Cellar` folder → `Brew`
/// 2. `~/.cx/install-method` contains "brew" → `Brew`
/// 3. `~/.cx/install-method` contains "curl" → `Curl`  (written by `install.sh`)
/// 4. Anything else → `Unknown`
///
/// The path comes first because the marker is one file shared by every copy
/// of `cx`. When a user has both a Homebrew `cx` and a script-installed `cx`,
/// `install.sh` writes "curl", and a marker-first check would tell the
/// Homebrew copy to rerun the script, which cannot upgrade it.
///
/// Windows-specific package managers (Scoop, Chocolatey, winget) are not yet
/// detected; Windows users get the `Unknown` fallback with a docs URL.
pub fn detect() -> InstallMethod {
    let exe = std::env::current_exe().ok().map(resolve_links);
    let marker = crate::config::config_dir()
        .ok()
        .and_then(|dir| std::fs::read_to_string(dir.join("install-method")).ok());
    detect_from(
        exe.as_deref().map(|p| p.to_string_lossy()).as_deref(),
        marker.as_deref(),
    )
}

fn detect_from(exe: Option<&str>, marker: Option<&str>) -> InstallMethod {
    if exe.is_some_and(is_brew_path) {
        return InstallMethod::Brew;
    }
    match marker.map(str::trim) {
        Some("brew") => InstallMethod::Brew,
        Some("curl") => InstallMethod::Curl,
        _ => InstallMethod::Unknown,
    }
}

/// Follow links to the real file. On macOS, `current_exe()` returns the path
/// used to start the program. On Intel Macs Homebrew's `cx` starts as
/// `/usr/local/bin/cx`, a link into `/usr/local/Cellar/`, and only the real
/// path shows it is Homebrew's.
fn resolve_links(path: std::path::PathBuf) -> std::path::PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

/// Homebrew keeps every formula's files under `<prefix>/Cellar/`, and its
/// `bin/` and `opt/` entries are links into it. Callers resolve links first,
/// so matching `/Cellar/` alone finds every Homebrew `cx`, and a plain file
/// that the script put in a Homebrew folder such as `/opt/homebrew/bin` is
/// not mistaken for one. Keep in sync with `is_brew_path` in `install.sh`.
fn is_brew_path(path: &str) -> bool {
    path.contains("/Cellar/")
}

/// Return the one-liner that upgrades the `cx` binary.
pub fn binary_upgrade_command(method: &InstallMethod) -> String {
    match method {
        InstallMethod::Brew => "brew upgrade cx".to_string(),
        InstallMethod::Curl => "curl -fsSL https://get.coralogix.dev/cli | sh".to_string(),
        InstallMethod::Unknown => "https://github.com/coralogix/cx-cli#install".to_string(),
    }
}

/// Return the one-liner that refreshes agent skills from the cx-cli repo.
///
/// Always passes `--global`: agents run cx without a terminal, where
/// `cx skills install` cannot ask for the scope, and global installs are the
/// only ones cx records and checks.
pub fn skills_upgrade_command() -> &'static str {
    "cx skills install --global"
}

/// Official install docs URL (binary + skills).
pub fn install_docs_url() -> &'static str {
    "https://github.com/coralogix/cx-cli#install"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brew_upgrade_command() {
        let cmd = binary_upgrade_command(&InstallMethod::Brew);
        assert_eq!(cmd, "brew upgrade cx");
    }

    #[test]
    fn curl_upgrade_command() {
        let cmd = binary_upgrade_command(&InstallMethod::Curl);
        assert!(cmd.contains("curl") && cmd.contains("coralogix.dev"));
    }

    #[test]
    fn unknown_upgrade_command_is_url() {
        let cmd = binary_upgrade_command(&InstallMethod::Unknown);
        assert_eq!(cmd, "https://github.com/coralogix/cx-cli#install");
    }

    #[test]
    fn skills_upgrade_command_is_global_install() {
        assert_eq!(skills_upgrade_command(), "cx skills install --global");
    }

    #[test]
    fn brew_binary_wins_over_curl_marker() {
        // Both installs exist; install.sh wrote "curl", but this is the
        // Homebrew copy, so it must suggest `brew upgrade cx`.
        let m = detect_from(
            Some("/opt/homebrew/Cellar/cx/0.1.28/bin/cx"),
            Some("curl\n"),
        );
        assert_eq!(m, InstallMethod::Brew);
    }

    #[test]
    fn script_binary_uses_curl_marker() {
        let m = detect_from(Some("/Users/me/.local/bin/cx"), Some("curl"));
        assert_eq!(m, InstallMethod::Curl);
    }

    #[test]
    fn linuxbrew_binary_without_marker_is_brew() {
        let m = detect_from(
            Some("/home/linuxbrew/.linuxbrew/Cellar/cx/0.1.28/bin/cx"),
            None,
        );
        assert_eq!(m, InstallMethod::Brew);
    }

    #[test]
    fn unknown_without_marker_or_brew_path() {
        assert_eq!(
            detect_from(Some("/usr/bin/cx"), None),
            InstallMethod::Unknown
        );
        assert_eq!(detect_from(None, Some("other")), InstallMethod::Unknown);
    }

    #[test]
    fn intel_homebrew_cellar_path_is_brew() {
        let m = detect_from(Some("/usr/local/Cellar/cx/0.1.28/bin/cx"), Some("curl"));
        assert_eq!(m, InstallMethod::Brew);
    }

    #[cfg(unix)]
    #[test]
    fn link_into_cellar_resolves_to_brew() {
        // Intel Mac layout: /usr/local/bin/cx -> ../Cellar/cx/<v>/bin/cx.
        let root = std::env::temp_dir().join(format!("cx-install-method-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let real_dir = root.join("Cellar/cx/0.1.28/bin");
        let bin_dir = root.join("bin");
        std::fs::create_dir_all(&real_dir).unwrap();
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(real_dir.join("cx"), "").unwrap();
        std::os::unix::fs::symlink("../Cellar/cx/0.1.28/bin/cx", bin_dir.join("cx")).unwrap();

        let link = bin_dir.join("cx");
        assert!(!is_brew_path(&link.to_string_lossy()));
        let resolved = resolve_links(link);
        let m = detect_from(Some(&resolved.to_string_lossy()), Some("curl"));
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(m, InstallMethod::Brew);
    }

    #[test]
    fn script_copy_in_homebrew_folder_uses_marker() {
        // CX_INSTALL_DIR=/opt/homebrew/bin: a plain file, not a Homebrew link.
        let m = detect_from(Some("/opt/homebrew/bin/cx"), Some("curl"));
        assert_eq!(m, InstallMethod::Curl);
    }
}
