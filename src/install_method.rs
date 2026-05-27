//! Detect how `cx` was installed so we can suggest the right upgrade command.

pub enum InstallMethod {
    Brew,
    Curl,
    Unknown,
}

/// Detect install method by inspecting the binary path or a marker file.
///
/// Order of checks:
/// 1. Binary lives in a Homebrew prefix → `Brew`
/// 2. `~/.cx/install-method` contains "curl" → `Curl`  (written by `install.sh`)
/// 3. Anything else → `Unknown`
///
/// Windows-specific package managers (Scoop, Chocolatey, winget) are not yet
/// detected; Windows users get the `Unknown` fallback with a docs URL.
pub fn detect() -> InstallMethod {
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

    if let Ok(cx_dir) = crate::config::config_dir() {
        let marker = cx_dir.join("install-method");
        if let Ok(content) = std::fs::read_to_string(marker) {
            if content.trim() == "curl" {
                return InstallMethod::Curl;
            }
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
        assert!(cmd.starts_with("https://"));
    }
}
