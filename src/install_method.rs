//! Detect how `cx` was installed so we can suggest the right upgrade command.

pub enum InstallMethod {
    Brew,
    Curl,
    Unknown,
}

/// Detect install method by reading a marker file or inspecting the binary path.
///
/// Order of checks:
/// 1. `~/.cx/install-method` contains "brew" → `Brew`  (written by Homebrew formula)
/// 2. `~/.cx/install-method` contains "curl" → `Curl`  (written by `install.sh`)
/// 3. Binary lives in a Homebrew prefix → `Brew`  (fallback for existing installs)
/// 4. Anything else → `Unknown`
///
/// Windows-specific package managers (Scoop, Chocolatey, winget) are not yet
/// detected; Windows users get the `Unknown` fallback with a docs URL.
pub fn detect() -> InstallMethod {
    // Prefer marker file (written by install.sh or Homebrew formula).
    if let Ok(cx_dir) = crate::config::config_dir() {
        let marker = cx_dir.join("install-method");
        if let Ok(content) = std::fs::read_to_string(marker) {
            match content.trim() {
                "brew" => return InstallMethod::Brew,
                "curl" => return InstallMethod::Curl,
                _ => {}
            }
        }
    }

    // Fallback: detect Homebrew by binary path (for users who installed before
    // the formula wrote the marker file).
    if let Ok(exe) = std::env::current_exe() {
        let path = exe.to_string_lossy();
        if path.contains("/opt/homebrew/")
            || path.contains("/usr/local/Cellar/")
            || path.contains("/usr/local/opt/")
            || path.contains("/home/linuxbrew/")
        {
            return InstallMethod::Brew;
        }
    }

    InstallMethod::Unknown
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
pub fn skills_upgrade_command() -> &'static str {
    "npx skills add coralogix/cx-cli/skills"
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
    fn skills_upgrade_command_is_npx() {
        assert_eq!(
            skills_upgrade_command(),
            "npx skills add coralogix/cx-cli/skills"
        );
    }
}
