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
/// 1. Binary lives in a Homebrew prefix → `Brew`
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
    let exe = std::env::current_exe().ok();
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

fn is_brew_path(path: &str) -> bool {
    path.contains("/opt/homebrew/")
        || path.contains("/usr/local/Cellar/")
        || path.contains("/usr/local/opt/")
        || path.contains("/home/linuxbrew/")
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
        let m = detect_from(Some("/home/linuxbrew/.linuxbrew/bin/cx"), None);
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
}
