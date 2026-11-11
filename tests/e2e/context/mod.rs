use std::sync::OnceLock;

use crate::harness;

const FLAG: &str = "CX_FEATURE_FLAG_AGENT_CONTEXT";
const REQUIRED: &str = "CX_E2E_CONTEXT_REQUIRED";
const NOT_AVAILABLE: &str = "Agent context is not available in this environment.";
const FORMATS: [&str; 3] = ["text", "json", "toon"];

#[derive(Debug, PartialEq)]
enum Probe {
    Available,
    Skip,
    Fail(String),
}

fn decide(success: bool, stderr: &str, required: bool) -> Probe {
    if success {
        Probe::Available
    } else if !stderr.contains(NOT_AVAILABLE) {
        Probe::Fail(format!(
            "availability probe `cx context get` failed:\n{stderr}"
        ))
    } else if required {
        Probe::Fail(format!(
            "{REQUIRED}=1 but agent context is not available:\n{stderr}"
        ))
    } else {
        Probe::Skip
    }
}

fn available(test_name: &str) -> bool {
    if harness::require_creds(test_name).is_none() {
        return false;
    }
    static PROBE: OnceLock<Probe> = OnceLock::new();
    let probe = PROBE.get_or_init(|| {
        let out = harness::cx()
            .env(FLAG, "1")
            .args(["context", "get", "-o", "json"])
            .output()
            .expect("cx should run");
        let required = std::env::var(REQUIRED).is_ok_and(|v| v == "1");
        decide(
            out.status.success(),
            &String::from_utf8_lossy(&out.stderr),
            required,
        )
    });
    match probe {
        Probe::Available => true,
        Probe::Skip => {
            eprintln!(
                "[e2e] skipping {test_name}: agent context is not available in this environment"
            );
            false
        }
        Probe::Fail(msg) => panic!("{msg}"),
    }
}

fn run_all_formats(test_name: &str, args: &[&str]) {
    if !available(test_name) {
        return;
    }
    std::env::set_var(FLAG, "1");
    for format in FORMATS {
        let mut full = args.to_vec();
        full.extend(["-o", format]);
        if format == "json" {
            harness::run_ok_json(&full);
        } else {
            harness::run_ok_nonempty(&full);
        }
    }
}

#[test]
#[ignore]
fn context_get() {
    run_all_formats("context_get", &["context", "get"]);
}

#[test]
#[ignore]
fn context_get_subject() {
    run_all_formats("context_get_subject", &["context", "get", "service:e2e"]);
}

#[test]
#[ignore]
fn context_memories_list() {
    run_all_formats("context_memories_list", &["context", "memories", "list"]);
}

#[test]
#[ignore]
fn context_instructions_list() {
    run_all_formats(
        "context_instructions_list",
        &["context", "instructions", "list"],
    );
}

#[test]
#[ignore]
fn context_memories_search() {
    run_all_formats(
        "context_memories_search",
        &["context", "memories", "search", "error"],
    );
}

#[test]
#[ignore]
fn context_history() {
    run_all_formats("context_history", &["context", "history"]);
}

// Writes (`create`, `update`, `delete`, `revert` for memories and
// instructions) are deliberately uncovered: they change shared team state
// and have no paired-undo plan.

#[test]
fn probe_success_is_available() {
    assert_eq!(decide(true, "", false), Probe::Available);
    assert_eq!(decide(true, "", true), Probe::Available);
}

#[test]
fn probe_not_available_skips() {
    assert_eq!(
        decide(false, &format!("Error: {NOT_AVAILABLE}"), false),
        Probe::Skip
    );
}

#[test]
fn probe_not_available_fails_when_required() {
    assert!(matches!(
        decide(false, &format!("Error: {NOT_AVAILABLE}"), true),
        Probe::Fail(_)
    ));
}

#[test]
fn probe_other_failure_fails() {
    assert!(matches!(
        decide(false, "Error: HTTP error: connection refused", false),
        Probe::Fail(_)
    ));
    assert!(matches!(
        decide(false, "Error: Authentication failed", true),
        Probe::Fail(_)
    ));
}
