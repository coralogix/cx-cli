use assert_cmd::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_home() -> std::path::PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("cx_wgate_test_{}_{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cx_agent(args: &[&str]) -> (bool, String) {
    let mut cmd = Command::cargo_bin("cx").expect("cx binary should build");
    cmd.env("CX_HOME", temp_home());
    cmd.env("CX_AGENT_MODE", "1");
    cmd.env_remove("CLAUDECODE");
    cmd.env_remove("CLAUDE_CODE");
    cmd.args(["--api-key", "fake", "--region", "us1"]);
    cmd.args(args);
    let output = cmd.output().expect("failed to run cx");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (!output.status.success(), stderr)
}

fn assert_gated(args: &[&str], label: &str) {
    let (failed, stderr) = cx_agent(args);
    assert!(
        failed && stderr.contains("requires user confirmation"),
        "{label}: expected confirmation gate, stderr: {stderr}"
    );
}

#[test]
fn iam_api_keys_delete() {
    assert_gated(
        &["iam", "api-keys", "delete", "nonexistent"],
        "iam api-keys delete",
    );
}

#[test]
fn iam_roles_delete() {
    assert_gated(
        &["iam", "roles", "delete", "nonexistent"],
        "iam roles delete",
    );
}

#[test]
fn iam_scopes_delete() {
    assert_gated(
        &["iam", "scopes", "delete", "nonexistent"],
        "iam scopes delete",
    );
}

#[test]
fn iam_users_set_status() {
    assert_gated(
        &[
            "iam",
            "users",
            "set-status",
            "--user-ids",
            "nonexistent",
            "--status",
            "active",
        ],
        "iam users set-status",
    );
}

#[test]
fn iam_team_groups_delete() {
    assert_gated(
        &["iam", "groups", "delete", "nonexistent"],
        "iam groups delete",
    );
}

#[test]
fn iam_ip_access_delete() {
    assert_gated(&["iam", "ip-access", "delete"], "iam ip-access delete");
}

#[test]
fn archive_metrics_enable() {
    assert_gated(&["archive", "metrics", "enable"], "archive metrics enable");
}

#[test]
fn archive_logs_set() {
    assert_gated(
        &["archive", "logs", "set", "--from-file", "/dev/null"],
        "archive logs set",
    );
}

#[test]
fn dashboards_create() {
    assert_gated(
        &["dashboards", "create", "--from-file", "/dev/null"],
        "dashboards create",
    );
}

#[test]
fn alerts_create() {
    assert_gated(
        &["alerts", "create", "--from-file", "/dev/null"],
        "alerts create",
    );
}

#[test]
fn alerts_suppression_rules_delete() {
    assert_gated(
        &["alerts", "suppression-rules", "delete", "nonexistent"],
        "alerts suppression-rules delete",
    );
}

#[test]
fn notifications_connectors_delete() {
    assert_gated(
        &["notifications", "connectors", "delete", "nonexistent"],
        "notifications connectors delete",
    );
}

#[test]
fn notifications_routers_delete() {
    assert_gated(
        &["notifications", "routers", "delete", "nonexistent"],
        "notifications routers delete",
    );
}

#[test]
fn notifications_presets_delete() {
    assert_gated(
        &["notifications", "presets", "delete", "nonexistent"],
        "notifications presets delete",
    );
}

#[test]
fn tco_delete() {
    assert_gated(&["tco", "delete", "nonexistent"], "tco delete");
}

#[test]
fn retentions_update() {
    assert_gated(
        &["retentions", "update", "--from-file", "/dev/null"],
        "retentions update",
    );
}

#[test]
fn e2m_delete() {
    assert_gated(&["e2m", "delete", "nonexistent"], "e2m delete");
}

#[test]
fn recording_rules_delete() {
    assert_gated(
        &["recording-rules", "delete", "nonexistent"],
        "recording-rules delete",
    );
}

#[test]
fn parsing_rules_delete() {
    assert_gated(
        &["parsing-rules", "delete", "nonexistent"],
        "parsing-rules delete",
    );
}

#[test]
fn enrichments_add() {
    assert_gated(
        &["enrichments", "add", "--from-file", "/dev/null"],
        "enrichments add",
    );
}

#[test]
fn enrichments_custom_delete() {
    assert_gated(
        &["enrichments", "custom", "delete", "nonexistent"],
        "enrichments custom delete",
    );
}

#[test]
fn integrations_delete() {
    assert_gated(
        &["integrations", "delete", "nonexistent"],
        "integrations delete",
    );
}

#[test]
fn integrations_extensions_deploy() {
    assert_gated(
        &[
            "integrations",
            "extensions",
            "deploy",
            "--from-file",
            "/dev/null",
        ],
        "integrations extensions deploy",
    );
}

#[test]
fn integrations_contextual_data_delete() {
    assert_gated(
        &["integrations", "contextual-data", "delete", "nonexistent"],
        "integrations contextual-data delete",
    );
}

#[test]
fn webhooks_delete() {
    assert_gated(&["webhooks", "delete", "nonexistent"], "webhooks delete");
}

#[test]
fn webhooks_actions_delete() {
    assert_gated(
        &["webhooks", "actions", "delete", "nonexistent"],
        "webhooks actions delete",
    );
}

#[test]
fn views_delete() {
    assert_gated(&["views", "delete", "nonexistent"], "views delete");
}

#[test]
fn views_folders_delete() {
    assert_gated(
        &["views", "folders", "delete", "nonexistent"],
        "views folders delete",
    );
}

#[test]
fn slos_delete() {
    assert_gated(&["slos", "delete", "nonexistent"], "slos delete");
}
