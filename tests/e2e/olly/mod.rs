//! `cx olly`: one paid, slow `ask` round-trip (skippable with
//! `CX_E2E_SKIP_OLLY=1`), the artifact commands, and the rules lifecycle
//! for team and personal rules.

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
        &[
            "id",
            "type",
            "chat_id",
            "chat_title",
            "chat_type",
            "interaction_id",
            "created_at",
        ],
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

/// A page of one, then every filter in one call built from that row: its
/// chat, its type and its own `created_at` as both time bounds. The row must
/// come back, which also proves the bounds are inclusive at full precision.
/// With two artifacts on the team, `--offset 1` must return the other one.
#[test]
#[ignore]
fn olly_artifacts_list_filters_and_pagination() {
    if harness::require_creds("olly_artifacts_list_filters_and_pagination").is_none() {
        return;
    }
    let page = harness::run_ok_json(&["olly", "artifacts", "list", "--limit", "1", "-o", "json"]);
    let page = harness::assert_array(&page);
    assert!(page.len() <= 1, "--limit 1 returned {} rows", page.len());

    let Some(first) = page.first() else {
        eprintln!("[e2e] olly artifacts list filters: no artifacts on test team");
        return;
    };
    let id = first["id"].as_str().expect("id");
    let chat_id = first["chat_id"].as_str().expect("chat_id");
    let artifact_type = first["type"].as_str().expect("type");
    let created_at = first["created_at"].as_str().expect("created_at");
    let filtered = harness::run_ok_json(&[
        "olly",
        "artifacts",
        "list",
        "--chat-id",
        chat_id,
        "--type",
        artifact_type,
        "--start",
        created_at,
        "--end",
        created_at,
        "--limit",
        "5",
        "--offset",
        "0",
        "-o",
        "json",
    ]);
    let filtered = harness::assert_array(&filtered);
    assert!(filtered.len() <= 5, "--limit 5 returned {}", filtered.len());
    assert!(
        filtered.iter().any(|row| row["id"] == id),
        "artifact {id} missing from its own chat, type and time window"
    );
    for row in filtered {
        assert_eq!(row["chat_id"], chat_id);
        assert_eq!(row["type"], artifact_type);
        assert_eq!(row["created_at"], created_at);
    }

    let second = harness::run_ok_json(&[
        "olly",
        "artifacts",
        "list",
        "--limit",
        "1",
        "--offset",
        "1",
        "-o",
        "json",
    ]);
    match harness::assert_array(&second).first() {
        Some(row) => assert_ne!(row["id"], id, "--offset 1 returned the first row again"),
        None => eprintln!("[e2e] olly artifacts list offset: only one artifact on test team"),
    }
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

/// list -> create -> get -> update -> disable -> enable -> delete, then `get`
/// must fail. `scope` is `&["--team"]` for team rules and empty for personal
/// ones.
fn rules_lifecycle(scope: &[&str]) {
    let label = if scope.is_empty() { "personal" } else { "team" };
    let with_scope = |args: &[&str]| -> Vec<String> {
        let mut full = vec!["olly".to_string(), "rules".to_string()];
        full.extend(args.iter().chain(scope).map(|s| s.to_string()));
        full
    };
    let run_json = |args: &[&str], write: bool| -> serde_json::Value {
        let mut full = with_scope(args);
        full.extend(["-o".to_string(), "json".to_string()]);
        let full: Vec<&str> = full.iter().map(String::as_str).collect();
        if write {
            harness::run_write_json(&full)
        } else {
            harness::run_ok_json(&full)
        }
    };

    // Reads need no write permission, so check the list shape first: it is
    // covered even when `create` is refused below. A team API key cannot
    // read personal rules at all.
    let listed = harness::cx()
        .args(with_scope(&["list", "-o", "json"]))
        .output()
        .expect("cx must run");
    let team_key = |stderr: &str| stderr.contains("user identity is required");
    if listed.status.success() {
        let v: serde_json::Value =
            serde_json::from_slice(&listed.stdout).expect("olly rules list -o json");
        harness::assert_array_of_objects_with_keys(&v, &["id", "rule", "enabled", "created_at"]);
    } else {
        let stderr = String::from_utf8_lossy(&listed.stderr);
        if team_key(&stderr) || stderr.contains("AI features are not enabled") {
            eprintln!(
                "[e2e] not covered: olly rules ({label}) - {}",
                harness::redact(stderr.trim())
            );
            return;
        }
        panic!("cx olly rules list ({label}) failed: {stderr}");
    }

    // The test key may lack olly-team-rules:Manage, which is a 403 on every
    // team write. Personal writes check no permission, so there a 403 fails.
    // The words after the unique name make the text read as an instruction
    // to the content check; the name still leads, so the sweeper finds it.
    let text = format!("{} answer briefly", harness::unique_name("olly-rule"));
    let mut create = with_scope(&["create", "--rule", &text]);
    create.extend(["--yes", "-o", "json"].map(String::from));
    let created = harness::cx().args(&create).output().expect("cx must run");
    if !created.status.success() {
        let stderr = String::from_utf8_lossy(&created.stderr);
        if stderr.contains("AI features are not enabled")
            || (!scope.is_empty() && stderr.contains("Permission denied"))
        {
            eprintln!(
                "[e2e] not covered: olly rules ({label}) create/update/enable/disable/delete - \
                 the API answered 403 for the test key ({})",
                harness::redact(stderr.trim())
            );
            return;
        }
        panic!("cx olly rules create ({label}) failed: {stderr}");
    }
    let rule: serde_json::Value = serde_json::from_slice(&created.stdout)
        .expect("olly rules create -o json must print valid JSON");
    harness::assert_object_with_keys(&rule, &["id", "rule", "enabled", "created_at"]);
    assert_eq!(rule["rule"], text, "{rule}");
    let id = rule["id"].as_str().expect("rule id").to_string();
    let delete = with_scope(&["delete", &id]);
    let delete: Vec<&str> = delete.iter().map(String::as_str).collect();
    let mut cleanup = harness::Cleanup::new(&delete);

    let list = run_json(&["list"], false);
    harness::assert_nonempty_array_of_objects_with_keys(
        &list,
        &["id", "rule", "enabled", "created_at"],
    );
    assert!(
        harness::assert_array(&list).iter().any(|r| r["id"] == id),
        "the new rule is listed: {list}"
    );
    for format in ["text", "toon"] {
        let mut args = with_scope(&["list"]);
        args.extend(["-o".to_string(), format.to_string()]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = harness::run_ok_nonempty(&args);
        assert!(String::from_utf8_lossy(&out).contains(&id), "{format} list");
    }

    let got = run_json(&["get", &id], false);
    assert_eq!(got["rule"], text, "{got}");
    let get_text = with_scope(&["get", &id]);
    let get_text: Vec<&str> = get_text.iter().map(String::as_str).collect();
    let out = harness::run_ok_nonempty(&get_text);
    assert!(String::from_utf8_lossy(&out).contains(&text));

    let new_text = format!("{} answer in detail", harness::unique_name("olly-rule"));
    let updated = run_json(&["update", &id, "--rule", &new_text], true);
    assert_eq!(updated["rule"], new_text, "{updated}");
    assert_eq!(updated["id"], id, "{updated}");

    let disabled = run_json(&["disable", &id], true);
    assert_eq!(disabled["enabled"], false, "{disabled}");
    assert_eq!(disabled["rule"], new_text, "disable keeps the text");
    // A disabled team rule is visible only with olly-team-rules:Manage. The
    // key that created it has that permission, so `get` should still work.
    let raw = harness::run_raw(&get_text);
    if !raw.status.success() {
        eprintln!(
            "[e2e] note: `olly rules get` ({label}) does not see a disabled rule with this \
             key; see \"To verify during implementation\" in HEA-87"
        );
    }

    let enabled = run_json(&["enable", &id], true);
    assert_eq!(enabled["enabled"], true, "{enabled}");

    harness::run_write_ok(&delete);
    cleanup.disarm();
    let stderr = harness::run_fail(&get_text);
    assert!(stderr.contains("404"), "{stderr}");
}

#[test]
#[ignore]
fn olly_team_rules_lifecycle() {
    if harness::require_write_creds("olly_team_rules_lifecycle").is_none() {
        return;
    }
    rules_lifecycle(&["--team"]);
}

#[test]
#[ignore]
fn olly_personal_rules_lifecycle() {
    if harness::require_write_creds("olly_personal_rules_lifecycle").is_none() {
        return;
    }
    rules_lifecycle(&[]);
}
