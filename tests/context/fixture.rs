use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicU32, Ordering};

use wiremock::MockServer;

pub const FLAG: &str = "CX_FEATURE_FLAG_AGENT_CONTEXT";

// Mirrors `safety::AGENT_ENV_VARS`, which is crate-private.
const AGENT_ENV_VARS: &[&str] = &[
    "AIDER",
    "AMAZON_Q",
    "AWS_Q_DEVELOPER",
    "CLAUDECODE",
    "CLAUDE_CODE",
    "CLINE",
    "CODEX",
    "COPILOT_AGENT",
    "CURSOR_AGENT",
    "CX_AGENT_MODE",
    "GEMINI_CODE_ASSIST",
    "GITHUB_COPILOT",
    "OPENAI_CODEX",
    "SRC_CODY",
    "WINDSURF_AGENT",
];

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh `CX_HOME`, removed on drop.
pub struct Home(PathBuf);

impl Home {
    pub fn new() -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("cx_context_test_{}_{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Home(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn write_profile(&self, name: &str, base_url: &str) {
        let profiles = self.0.join(".cx").join("profiles");
        std::fs::create_dir_all(&profiles).unwrap();
        std::fs::write(
            profiles.join(format!("{name}.toml")),
            format!(
                "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"test\"\nregion = \"{base_url}\"\n"
            ),
        )
        .unwrap();
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The real binary with a controlled environment: no profile, read-only,
/// timeout, completion or agent-detection variables, and an empty stdin.
pub fn cx(home: &Home, flag: bool) -> assert_cmd::Command {
    controlled(assert_cmd::cargo::cargo_bin("cx"), home, flag)
}

/// `sh -c script` with the same environment as [`cx`]; `$CX` is the binary.
pub fn shell(home: &Home, script: &str) -> Output {
    controlled("sh".into(), home, false)
        .env("CX", assert_cmd::cargo::cargo_bin("cx"))
        .args(["-c", script])
        .output()
        .expect("failed to run sh")
}

fn controlled(program: PathBuf, home: &Home, flag: bool) -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::new(program);
    cmd.env("CX_HOME", home.path())
        .env("CX_NO_UPDATE_NOTIFIER", "1")
        .env_remove("CX_PROFILE")
        .env_remove("CX_API_KEY")
        .env_remove("CX_REGION")
        .env_remove("CX_READ_ONLY")
        .env_remove("CX_HTTP_TIMEOUT")
        .env_remove("COMPLETE")
        .write_stdin("");
    for var in AGENT_ENV_VARS {
        cmd.env_remove(var);
    }
    if flag {
        cmd.env(FLAG, "1");
    } else {
        cmd.env_remove(FLAG);
    }
    cmd
}

/// Runs `cx --api-key test --region <server> -o <format> <args>`.
pub fn run(home: &Home, server: &MockServer, format: &str, args: &[&str]) -> Output {
    cx(home, false)
        .args(["--api-key", "test", "--region", &server.uri(), "-o", format])
        .args(args)
        .output()
        .expect("failed to run cx")
}

/// Dynamic completion candidates for `cx <prefix>`.
pub fn completion_candidates(home: &Home, flag: bool, prefix: &str) -> Vec<String> {
    let output = cx(home, flag)
        .env("COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", "1")
        .args(["--", "cx", prefix])
        .output()
        .expect("failed to run cx completion");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

pub async fn request_count(server: &MockServer) -> usize {
    server.received_requests().await.map_or(0, |r| r.len())
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}
