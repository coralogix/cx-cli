use std::env;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const RELEASE_NOTICE: &str =
    "cx sends usage telemetry by default. Opt out with CX_TELEMETRY=off, DO_NOT_TRACK=1, or telemetry=false in ~/.cx/config.toml.";
const NOTICE_MARKER_FILE: &str = "release-default-notice-shown";
const NOTICE_MARKER_CONTENT: &[u8] = b"shown-v1\n";
const NOTICE_LOCK_FILE: &str = "release-default-notice.lock";
const LOCK_TIMEOUT: Duration = Duration::from_secs(1);
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(5);
const MAX_MARKER_BYTES: usize = 32;

pub(super) fn allow_collection_after_notice() -> bool {
    if !implicit_release_default_consent() {
        return true;
    }

    let notice_dir = notice_directory().ok();
    let lock = notice_dir
        .as_ref()
        .and_then(|dir| lock_notice_state(dir).ok());
    if let Some(dir) = notice_dir.as_ref().filter(|_| lock.is_some()) {
        if marker_is_complete(dir) {
            return true;
        }
    }

    if write_notice().is_err() {
        return false;
    }
    if let Some(dir) = notice_dir.as_ref().filter(|_| lock.is_some()) {
        let _ = crate::atomic_file::write(&dir.join(NOTICE_MARKER_FILE), NOTICE_MARKER_CONTENT);
    }
    true
}

fn lock_notice_state(directory: &PathBuf) -> io::Result<File> {
    ensure_private_directory(directory)?;
    let path = directory.join(NOTICE_LOCK_FILE);
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    let deadline = Instant::now() + LOCK_TIMEOUT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(io::Error::new(
                        ErrorKind::TimedOut,
                        "notice marker lock is contended",
                    ));
                }
                std::thread::sleep(remaining.min(LOCK_RETRY_INTERVAL));
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
    }
}

fn marker_is_complete(directory: &Path) -> bool {
    let path = directory.join(NOTICE_MARKER_FILE);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return false,
        Err(_) => return false,
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_MARKER_BYTES as u64 {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return false;
        }
    }
    let mut bytes = Vec::new();
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    if file
        .take(MAX_MARKER_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return false;
    }
    bytes == NOTICE_MARKER_CONTENT
}

fn notice_directory() -> io::Result<PathBuf> {
    crate::config::config_dir()
        .map(|path| path.join("telemetry-notice"))
        .map_err(io::Error::other)
}

fn ensure_private_directory(path: &PathBuf) -> io::Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

fn write_notice() -> io::Result<()> {
    #[cfg(debug_assertions)]
    if env::var("CX_TELEMETRY_TEST_FAIL_NOTICE_STDERR").is_ok_and(|value| super::truthy(&value)) {
        return Err(io::Error::from(ErrorKind::BrokenPipe));
    }
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "{RELEASE_NOTICE}")
}

fn implicit_release_default_consent() -> bool {
    let forced = force_release_notice_for_tests();
    if !release_notice_enabled()
        || (explicit_destination_is_configured() && !forced)
        || cx_telemetry_env_is_explicit()
        || do_not_track_opt_out()
    {
        return false;
    }
    matches!(
        crate::config::telemetry_consent(),
        crate::config::TelemetryConsent::Implicit
    )
}

fn explicit_destination_is_configured() -> bool {
    ["CX_TELEMETRY_ENDPOINT", "CX_TELEMETRY_KEY_URL"]
        .into_iter()
        .any(|key| {
            env::var(key)
                .ok()
                .is_some_and(|value| !value.trim().is_empty())
        })
}

fn cx_telemetry_env_is_explicit() -> bool {
    env::var("CX_TELEMETRY")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn do_not_track_opt_out() -> bool {
    env::var("DO_NOT_TRACK")
        .ok()
        .is_some_and(|value| super::truthy(&value))
}

fn release_notice_enabled() -> bool {
    if !cfg!(debug_assertions) {
        return true;
    }
    force_release_notice_for_tests()
}

fn force_release_notice_for_tests() -> bool {
    #[cfg(debug_assertions)]
    {
        let requested = env::var("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE");
        requested.is_ok_and(|value| super::truthy(&value))
    }
    #[cfg(not(debug_assertions))]
    {
        false
    }
}
