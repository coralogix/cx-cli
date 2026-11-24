#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs::File;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::io::Read;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::Path;

const MAX_VERSION_LEN: usize = 32;
#[cfg(target_os = "linux")]
const LINUX_OSRELEASE_MAX_BYTES: usize = 128;
#[cfg(target_os = "macos")]
const MACOS_SYSTEM_VERSION_MAX_BYTES: usize = 16 * 1024;

#[cfg(target_os = "linux")]
pub(super) fn os_version() -> Option<String> {
    linux_os_version()
}

#[cfg(target_os = "macos")]
pub(super) fn os_version() -> Option<String> {
    macos_os_version()
}

#[cfg(target_os = "windows")]
pub(super) fn os_version() -> Option<String> {
    let version = os_info::get().version().to_string();
    normalize_version(&version)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(super) fn os_version() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn linux_os_version() -> Option<String> {
    let bytes = read_bounded("/proc/sys/kernel/osrelease", LINUX_OSRELEASE_MAX_BYTES)?;
    let release = std::str::from_utf8(&bytes).ok()?.trim();
    normalize_version(linux_numeric_prefix(release)?)
}

#[cfg(target_os = "macos")]
fn macos_os_version() -> Option<String> {
    let bytes = read_bounded(
        "/System/Library/CoreServices/SystemVersion.plist",
        MACOS_SYSTEM_VERSION_MAX_BYTES,
    )?;
    let plist = std::str::from_utf8(&bytes).ok()?;
    normalize_version(&product_version_from_plist(plist)?)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn read_bounded(path: impl AsRef<Path>, max_bytes: usize) -> Option<Vec<u8>> {
    let metadata = std::fs::metadata(path.as_ref()).ok()?;
    if metadata.len() > max_bytes as u64 {
        return None;
    }
    let file = File::open(path).ok()?;
    let mut buffer = Vec::new();
    file.take((max_bytes + 1) as u64)
        .read_to_end(&mut buffer)
        .ok()?;
    (buffer.len() <= max_bytes).then_some(buffer)
}

#[cfg(any(target_os = "linux", test))]
fn linux_numeric_prefix(release: &str) -> Option<&str> {
    let end = release
        .as_bytes()
        .iter()
        .take_while(|byte| byte.is_ascii_digit() || **byte == b'.')
        .count();
    (end > 0).then_some(&release[..end])
}

#[cfg(any(target_os = "macos", test))]
fn product_version_from_plist(plist: &str) -> Option<String> {
    let key = "<key>ProductVersion</key>";
    let string_start = "<string>";
    let string_end = "</string>";
    let key_index = plist.find(key)?;
    let after_key = plist[key_index + key.len()..].trim_start();
    let after_value_start = after_key.strip_prefix(string_start)?;
    let value_end = after_value_start.find(string_end)?;
    Some(after_value_start[..value_end].trim().to_owned())
}

fn normalize_version(value: &str) -> Option<String> {
    let trimmed = value.trim();
    is_numeric_dotted(trimmed).then(|| trimmed.to_owned())
}

fn is_numeric_dotted(value: &str) -> bool {
    value.len() <= MAX_VERSION_LEN
        && value
            .split('.')
            .all(|component| !component.is_empty() && component.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_numeric_dotted_versions() {
        assert!(is_numeric_dotted("6.8.0"));
        assert!(is_numeric_dotted("14.7.1"));
        assert!(!is_numeric_dotted(""));
        assert!(!is_numeric_dotted("6.8."));
        assert!(!is_numeric_dotted("6..8"));
        assert!(!is_numeric_dotted("6.8-rc1"));
        assert!(!is_numeric_dotted("v14.7"));
        assert!(!is_numeric_dotted(&"1".repeat(33)));
    }

    #[test]
    fn keeps_linux_numeric_prefix_before_suffix() {
        assert_eq!(linux_numeric_prefix("6.8.0-52-generic"), Some("6.8.0"));
        assert_eq!(linux_numeric_prefix("6.8.0"), Some("6.8.0"));
        assert_eq!(linux_numeric_prefix("release-6.8.0"), None);
    }

    #[test]
    fn parses_product_version_from_plist() {
        let plist = r#"
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
    <key>ProductName</key>
    <string>macOS</string>
    <key>ProductVersion</key>
    <string>14.7.1</string>
</dict>
</plist>
"#;
        assert_eq!(product_version_from_plist(plist).as_deref(), Some("14.7.1"));
    }

    #[test]
    fn plist_missing_or_malformed_product_version_is_rejected() {
        let missing = "<dict><key>ProductName</key><string>macOS</string></dict>";
        let malformed = "<dict><key>ProductVersion</key><string>14.7.1</dict>";
        let wrong_next_value = r#"
<dict>
    <key>ProductVersion</key>
    <key>ProductName</key>
    <string>macOS</string>
</dict>
"#;
        assert!(product_version_from_plist(missing).is_none());
        assert!(product_version_from_plist(malformed).is_none());
        assert!(product_version_from_plist(wrong_next_value).is_none());
    }

    #[test]
    fn normalize_version_rejects_non_numeric_values() {
        assert_eq!(normalize_version(" 14.7.1 ").as_deref(), Some("14.7.1"));
        assert!(normalize_version("14.7-beta").is_none());
        assert!(normalize_version("darwin-23.6.0").is_none());
    }
}
