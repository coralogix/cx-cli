//! `cx olly`: one paid, slow `ask` round-trip (skippable with
//! `CX_E2E_SKIP_OLLY=1`) and the artifact commands.

use crate::harness;

fn skip_olly_ask(test_name: &str) -> bool {
    if std::env::var_os("CX_E2E_SKIP_OLLY").is_some() {
        eprintln!("[e2e] skipping {test_name}: CX_E2E_SKIP_OLLY is set");
        return true;
    }
    false
}

/// One short ask that drives every flag: a new chat with --model/--timeout/
/// --agent-to-agent-mode in JSON mode, then a follow-up on the same chat
/// via --chat-id in text mode. Both prompts ask for a one-word reply to keep
/// the cost and the wall time down.
#[test]
#[ignore]
fn olly_ask_round_trip_with_all_flags() {
    if harness::require_creds("olly_ask_round_trip_with_all_flags").is_none()
        || skip_olly_ask("olly_ask_round_trip_with_all_flags")
    {
        return;
    }
    let args = [
        "olly",
        "ask",
        "Reply with the single word OK and nothing else.",
        "--model",
        "gpt-5.2",
        "--timeout",
        "600",
        "--agent-to-agent-mode",
        "-o",
        "json",
    ];
    // Olly is a per-team entitlement. A team without it answers 403 before a
    // chat is created; that is the API's answer, not a CLI defect, and it
    // leaves nothing else to assert on.
    let first_try = harness::cx().args(args).output().expect("cx must run");
    if !first_try.status.success() {
        let stderr = String::from_utf8_lossy(&first_try.stderr);
        if stderr.contains("AI features are not enabled") {
            eprintln!(
                "[e2e] not covered: olly ask - AI features are not enabled for the test team \
                 (API answered: {})",
                stderr.trim()
            );
            return;
        }
        panic!("cx olly ask failed: {stderr}");
    }
    let v: serde_json::Value =
        serde_json::from_slice(&first_try.stdout).expect("olly ask -o json must print valid JSON");
    harness::assert_nonempty_array_of_objects_with_keys(
        &v,
        &["chat_id", "interaction_id", "status"],
    );
    let first = &v[0];
    let chat_id = first["chat_id"]
        .as_str()
        .expect("chat_id is a string")
        .to_string();
    assert!(!chat_id.is_empty());
    assert!(first["interaction_id"].is_string(), "{first}");
    assert!(
        first["response"]
            .as_str()
            .is_some_and(|r| !r.trim().is_empty()),
        "a completed ask carries the assistant text: {first}"
    );

    let text = harness::run_ok_nonempty(&[
        "olly",
        "ask",
        "Reply with the single word OK again.",
        "--chat-id",
        &chat_id,
        "--timeout",
        "600",
        "-o",
        "text",
    ]);
    let text = String::from_utf8_lossy(&text);
    assert!(
        text.contains("Chat ID:"),
        "text output names the chat: {text}"
    );
    assert!(
        text.contains(&chat_id),
        "--chat-id continues the same chat: {text}"
    );
}

#[test]
#[ignore]
fn olly_artifacts_list() {
    if harness::require_creds("olly_artifacts_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["olly", "artifacts", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(
        &v,
        &["id", "download_url", "filename", "content_type", "size"],
    );
    let text = harness::run_ok_nonempty(&["olly", "artifacts", "list"]);
    let text = String::from_utf8_lossy(&text);
    if v.as_array().unwrap().is_empty() {
        assert!(text.contains("No artifacts found"), "{text}");
    } else {
        assert!(text.contains("ID"), "{text}");
    }
    harness::run_ok(&["olly", "artifacts", "list", "-o", "toon"]);
}

/// `get` on a real artifact when the team has one; a made-up id is a 404
/// either way.
#[test]
#[ignore]
fn olly_artifacts_get() {
    if harness::require_creds("olly_artifacts_get").is_none() {
        return;
    }
    let list = harness::run_ok_json(&["olly", "artifacts", "list", "-o", "json"]);
    match list.as_array().and_then(|a| a.first()) {
        Some(first) => {
            let id = first["id"].as_str().expect("artifact id").to_string();
            let v = harness::run_ok_json(&["olly", "artifacts", "get", &id, "-o", "json"]);
            harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "filename"]);
            assert_eq!(v[0]["id"], id);
            harness::run_ok_nonempty(&["olly", "artifacts", "get", &id]);
        }
        None => {
            eprintln!("[e2e] olly artifacts get: no artifacts on test team, only the 404 path runs")
        }
    }

    let output = harness::cx()
        .args([
            "olly",
            "artifacts",
            "get",
            "00000000-0000-0000-0000-000000000000",
            "-o",
            "json",
        ])
        .output()
        .expect("cx must run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("404"), "{stderr}");
}

/// Scheduled tasks need a user identity and Olly on the team; a team key or a
/// team without Olly answers before anything is created.
fn scheduled_tasks_unavailable(stderr: &str) -> bool {
    stderr.contains("AI features are not enabled")
        || stderr.contains("A user identity is required")
        || stderr.contains("Team API keys are not supported")
}

/// create (paused, so it never runs) -> get -> list -> update -> share ->
/// runs -> delete -> get fails.
#[test]
#[ignore]
fn olly_scheduled_tasks_lifecycle() {
    if harness::require_write_creds("olly_scheduled_tasks_lifecycle").is_none() {
        return;
    }
    let title = harness::unique_name("scheduled-task");
    let create = harness::run_raw(&[
        "olly",
        "scheduled-tasks",
        "create",
        "--title",
        &title,
        "--prompt",
        "Reply with the single word OK.",
        "--every",
        "1d",
        "--start-at",
        "now+1d",
        "--disabled",
        "-o",
        "json",
        "--yes",
    ]);
    if !create.status.success() {
        let stderr = String::from_utf8_lossy(&create.stderr);
        if scheduled_tasks_unavailable(&stderr) {
            eprintln!(
                "[e2e] not covered: olly scheduled-tasks ({})",
                stderr.trim()
            );
            return;
        }
        panic!("cx olly scheduled-tasks create failed: {stderr}");
    }
    let created: serde_json::Value =
        serde_json::from_slice(&create.stdout).expect("create -o json must print valid JSON");
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("create returned no id: {created}"));
    let mut cleanup = harness::Cleanup::new(&["olly", "scheduled-tasks", "delete", &id]);
    assert_eq!(created["title"], title.as_str());
    assert_eq!(created["enabled"], false);
    assert_eq!(created["interval_seconds"], 86400);

    let got = harness::run_ok_json(&["olly", "scheduled-tasks", "get", &id, "-o", "json"]);
    assert_eq!(got["id"], id.as_str());
    assert_eq!(got["is_owner"], true);

    let list = harness::run_ok_json(&["olly", "scheduled-tasks", "list", "-o", "json"]);
    harness::find_by_name(&list, "title", &title)
        .unwrap_or_else(|| panic!("list should contain {title}"));

    let updated = harness::run_write_json(&[
        "olly",
        "scheduled-tasks",
        "update",
        &id,
        "--title",
        &format!("{title}-upd"),
        "--every",
        "6h",
        "-o",
        "json",
    ]);
    assert_eq!(updated["interval_seconds"], 21600, "{updated}");
    assert_eq!(
        updated["enabled"], false,
        "update must keep it paused: {updated}"
    );

    let shared = harness::run_write_json(&[
        "olly",
        "scheduled-tasks",
        "share",
        &id,
        "--team",
        "-o",
        "json",
    ]);
    assert_eq!(
        shared.pointer("/shared_options/shared_type"),
        Some(&serde_json::json!("entire_organization"))
    );

    let runs = harness::run_ok_json(&["olly", "scheduled-tasks", "runs", &id, "-o", "json"]);
    harness::assert_array(&runs);
    harness::run_ok_nonempty(&["olly", "scheduled-tasks", "get", &id]);

    harness::run_write_ok(&["olly", "scheduled-tasks", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["olly", "scheduled-tasks", "get", &id]);
}
