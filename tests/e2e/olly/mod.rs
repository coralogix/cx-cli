//! `cx olly`: one paid, slow `ask` round-trip (skippable with
//! `CX_E2E_SKIP_OLLY=1`), the artifact commands, and the read-only chat and
//! model commands.

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

#[test]
#[ignore]
fn olly_models() {
    if harness::require_creds("olly_models").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["olly", "models", "-o", "json"]);
    harness::assert_array_of_strings(&v);
    assert!(!v.as_array().unwrap().is_empty(), "no models listed");
    let text = harness::run_ok_nonempty(&["olly", "models"]);
    let text = String::from_utf8_lossy(&text);
    assert_eq!(text.lines().count(), v.as_array().unwrap().len(), "{text}");
    harness::run_ok(&["olly", "models", "-o", "toon"]);
}

/// `list` with paging and a type filter, then `get` on a listed chat when the
/// team has one; a made-up id is a 404 either way.
#[test]
#[ignore]
fn olly_chats_list_and_get() {
    if harness::require_creds("olly_chats_list_and_get").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["olly", "chats", "list", "--limit", "2", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "title", "type", "created_at"]);
    let chats = v.as_array().unwrap();
    assert!(chats.len() <= 2, "--limit 2 returned {}", chats.len());

    let text = harness::run_ok_nonempty(&["olly", "chats", "list", "--limit", "2"]);
    let text = String::from_utf8_lossy(&text);
    if chats.is_empty() {
        assert!(text.contains("No chats found"), "{text}");
    } else {
        assert!(text.contains("TITLE"), "{text}");
    }
    harness::run_ok(&["olly", "chats", "list", "--limit", "2", "-o", "toon"]);

    let typed = harness::run_ok_json(&[
        "olly",
        "chats",
        "list",
        "--type",
        "ai_connector",
        "--limit",
        "5",
        "--offset",
        "1",
        "-o",
        "json",
    ]);
    for chat in harness::assert_array(&typed) {
        assert_eq!(chat["type"], "ai_connector", "{chat}");
    }

    match chats.first() {
        Some(first) => {
            let id = first["id"].as_str().expect("chat id").to_string();
            let chat = harness::run_ok_json(&["olly", "chats", "get", &id, "-o", "json"]);
            harness::assert_object_with_keys(&chat, &["id", "messages", "interactions"]);
            assert_eq!(chat["id"], id);
            let text = harness::run_ok_nonempty(&["olly", "chats", "get", &id]);
            assert!(String::from_utf8_lossy(&text).contains(&id));
        }
        None => eprintln!("[e2e] olly chats get: no chats on test team, only the 404 path runs"),
    }

    let stderr = harness::run_fail(&[
        "olly",
        "chats",
        "get",
        "00000000-0000-0000-0000-000000000000",
    ]);
    assert!(stderr.contains("404"), "{stderr}");
}
