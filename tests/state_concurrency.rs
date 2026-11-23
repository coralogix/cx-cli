use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use coralogix_cli::version_cache::{self, VersionCheckCache};
use serde_json::{json, Value};

const WORKER: &str = "CX_TEST_STATE_WORKER";
const PROCESS_TIMEOUT: Duration = Duration::from_secs(15);
const SNAPSHOTS: usize = 48;

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("cx-state-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn state_path(&self) -> PathBuf {
        self.0.join(".cx/state.json")
    }

    fn worker(&self, action: &str) -> Child {
        Command::new(std::env::current_exe().unwrap())
            .env("CX_HOME", &self.0)
            .env(WORKER, action)
            .args(["--ignored", "--exact", "state_worker"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn start(&self, workers: &[&Child]) {
        wait_until(|| {
            workers
                .iter()
                .all(|child| self.0.join(format!("ready-{}", child.id())).exists())
        });
        fs::write(self.0.join("start"), []).unwrap();
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + PROCESS_TIMEOUT;
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "state worker did not finish in time"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "state worker failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn finish(mut child: Child) {
    wait_until(|| child.try_wait().unwrap().is_some());
    assert_success(child.wait_with_output().unwrap());
}

fn seeded_time() -> DateTime<Utc> {
    DateTime::from_timestamp(1_766_000_000, 0).unwrap()
}

fn snapshot_payload(index: usize) -> String {
    format!("{index:02}").repeat(32 * 1024)
}

#[test]
#[ignore = "invoked by the cross-process state cache tests"]
fn state_worker() {
    let action = std::env::var(WORKER).unwrap();
    let home = PathBuf::from(std::env::var_os("CX_HOME").unwrap());
    let pid = std::process::id();
    fs::write(home.join(format!("ready-{pid}")), []).unwrap();
    wait_until(|| home.join("start").exists());

    match action.as_str() {
        "identity" => {
            let identity = version_cache::installation_id();
            fs::write(home.join(format!("id-{pid}")), identity).unwrap();
        }
        "skills-install" => version_cache::record_global_skills_install(&["cursor".into()]),
        "skills-refresh" => version_cache::record_skills_refresh_attempt("refresh-version"),
        "skills-notice" => version_cache::record_skills_unknown_noticed("notice-version"),
        "snapshots" => {
            for index in 1..=SNAPSHOTS {
                VersionCheckCache {
                    latest_binary: Some(index.to_string()),
                    skills_agents: vec![snapshot_payload(index)],
                    ..Default::default()
                }
                .save()
                .unwrap();
            }
        }
        _ => panic!("unknown state worker action {action}"),
    }
}

#[test]
fn concurrent_processes_share_one_identity_and_preserve_other_state() {
    let sandbox = Sandbox::new();
    let path = sandbox.state_path();
    fs::create_dir(path.parent().unwrap()).unwrap();
    let seed = VersionCheckCache {
        latest_binary: Some("cached-version".into()),
        last_checked_at: Some(seeded_time()),
        ..Default::default()
    };
    let future_metadata = json!({"release": {"generations": [1, null, "sentinel"]}});
    let mut seed_json = serde_json::to_value(&seed).unwrap();
    seed_json["future_metadata"] = future_metadata.clone();
    fs::write(&path, serde_json::to_vec(&seed_json).unwrap()).unwrap();

    let identities: Vec<_> = (0..8).map(|_| sandbox.worker("identity")).collect();
    let identity_paths: Vec<_> = identities
        .iter()
        .map(|child| sandbox.0.join(format!("id-{}", child.id())))
        .collect();
    let mutations: Vec<_> = ["skills-install", "skills-refresh", "skills-notice"]
        .into_iter()
        .map(|action| sandbox.worker(action))
        .collect();
    sandbox.start(&identities.iter().chain(&mutations).collect::<Vec<_>>());
    for child in identities.into_iter().chain(mutations) {
        finish(child);
    }

    let persisted_json: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(persisted_json["future_metadata"], future_metadata);
    let state: VersionCheckCache = serde_json::from_value(persisted_json).unwrap();
    let persisted = state.installation_id.unwrap();
    assert!(uuid::Uuid::parse_str(&persisted).is_ok());
    for identity in identity_paths {
        assert_eq!(fs::read_to_string(identity).unwrap(), persisted);
    }
    assert_eq!(state.latest_binary, seed.latest_binary);
    assert_eq!(state.last_checked_at, seed.last_checked_at);
    assert_eq!(
        state.skills_installed_for.as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(state.skills_agents, ["cursor"]);
    assert_eq!(
        state.skills_refresh_attempted_for.as_deref(),
        Some("refresh-version")
    );
    assert!(state.skills_refresh_attempted_at.is_some());
    assert_eq!(
        state.skills_unknown_noticed_for.as_deref(),
        Some("notice-version")
    );
}

#[test]
fn concurrent_reader_sees_complete_json_snapshots() {
    let sandbox = Sandbox::new();
    let path = sandbox.state_path();
    fs::create_dir(path.parent().unwrap()).unwrap();
    let seed = VersionCheckCache {
        latest_binary: Some("0".into()),
        skills_agents: vec![snapshot_payload(0)],
        ..Default::default()
    };
    fs::write(&path, serde_json::to_vec(&seed).unwrap()).unwrap();

    let mut writer = sandbox.worker("snapshots");
    sandbox.start(&[&writer]);
    let mut reads = 0;
    wait_until(|| {
        let bytes = fs::read(&path).unwrap();
        let state: VersionCheckCache =
            serde_json::from_slice(&bytes).expect("reader observed incomplete JSON");
        let index = state.latest_binary.unwrap().parse().unwrap();
        assert_eq!(
            state.skills_agents,
            [snapshot_payload(index)],
            "mixed snapshot fields"
        );
        reads += 1;
        writer.try_wait().unwrap().is_some()
    });
    assert_success(writer.wait_with_output().unwrap());
    assert!(reads > 1, "reader must overlap the writer");
    let state: VersionCheckCache = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(state.latest_binary, Some(SNAPSHOTS.to_string()));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn contended_state_write_returns_without_modifying_state() {
    let sandbox = Sandbox::new();
    let path = sandbox.state_path();
    fs::create_dir(path.parent().unwrap()).unwrap();
    let seed = br#"{"latest_binary":"locked"}"#;
    fs::write(&path, seed).unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path.with_extension("lock"))
        .unwrap();
    lock.try_lock().unwrap();

    let worker = sandbox.worker("skills-install");
    sandbox.start(&[&worker]);
    finish(worker);
    assert_eq!(fs::read(path).unwrap(), seed);
    drop(lock);
}
