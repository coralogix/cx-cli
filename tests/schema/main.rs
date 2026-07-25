use serde_json::Value;

#[path = "../../src/main.rs"]
#[allow(dead_code)]
mod main_mod;

// Can't include main.rs directly due to #[tokio::main], so test via the binary.

#[test]
fn schema_outputs_valid_json_with_expected_commands() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cx"))
        .arg("schema")
        .output()
        .expect("failed to run cx schema");

    assert!(output.status.success(), "cx schema failed");

    let schema: Value =
        serde_json::from_slice(&output.stdout).expect("schema output is not valid JSON");

    let commands = schema["commands"]
        .as_array()
        .expect("commands should be an array");
    assert_eq!(commands.len(), 29, "expected 29 top-level commands");

    let names: Vec<&str> = commands
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();

    // Verify key merged/renamed commands exist
    assert!(names.contains(&"alerts"), "missing alerts");
    assert!(names.contains(&"cases"), "missing cases");
    assert!(names.contains(&"iam"), "missing iam");
    assert!(names.contains(&"notifications"), "missing notifications");
    assert!(names.contains(&"webhooks"), "missing webhooks");
    assert!(names.contains(&"enrichments"), "missing enrichments");
    assert!(names.contains(&"integrations"), "missing integrations");
    assert!(names.contains(&"parsing-rules"), "missing parsing-rules");
    assert!(names.contains(&"tco"), "missing tco");
    assert!(names.contains(&"usage"), "missing usage");
    assert!(names.contains(&"archive"), "missing archive");
    assert!(names.contains(&"schema"), "missing schema");
    assert!(names.contains(&"docs"), "missing docs");
    assert!(names.contains(&"olly"), "missing olly");
    assert!(names.contains(&"ai-center"), "missing ai-center");

    // Verify old commands are gone
    assert!(
        !names.contains(&"search-by-value"),
        "search-by-value merged into search-fields"
    );
    assert!(!names.contains(&"alert-schedulers"));
    assert!(!names.contains(&"actions"));
    assert!(!names.contains(&"custom-enrichments"));
    assert!(!names.contains(&"connectors"));
    assert!(!names.contains(&"routers"));
    assert!(!names.contains(&"presets"));
    assert!(!names.contains(&"api-keys"));
    assert!(!names.contains(&"rule-groups"));
    assert!(!names.contains(&"tco-policies"));
    assert!(!names.contains(&"incidents"), "incidents was removed");

    // Verify alerts subcommands include schedulers
    let alerts = commands.iter().find(|c| c["name"] == "alerts").unwrap();
    let alert_subs: Vec<&str> = alerts["subcommands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(alert_subs.contains(&"list"));
    assert!(alert_subs.contains(&"suppression-rules"));

    // Verify iam subcommands
    let iam = commands.iter().find(|c| c["name"] == "iam").unwrap();
    let iam_subs: Vec<&str> = iam["subcommands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(iam_subs.contains(&"api-keys"));
    assert!(iam_subs.contains(&"roles"));
    assert!(iam_subs.contains(&"scopes"));
    assert!(iam_subs.contains(&"users"));
    assert!(iam_subs.contains(&"groups"));
    assert!(iam_subs.contains(&"ip-access"));

    // Verify docs subcommands
    let docs = commands.iter().find(|c| c["name"] == "docs").unwrap();
    let docs_subs: Vec<&str> = docs["subcommands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(docs_subs.contains(&"search"));
    assert!(docs_subs.contains(&"fetch"));

    // Regression (FORGE-125): schema must distinguish options from positionals
    // so agents build commands the CLI accepts.
    let alerts_subs = alerts["subcommands"].as_array().unwrap();

    // `alerts list --name` is an option, not a positional.
    let list = alerts_subs.iter().find(|s| s["name"] == "list").unwrap();
    let name_arg = list["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "name")
        .expect("alerts list should expose a 'name' argument");
    assert_eq!(
        name_arg["positional"], false,
        "alerts list --name must be reported as an option, not positional"
    );
    assert_eq!(
        name_arg["flag"], "--name",
        "alerts list --name must advertise its flag string"
    );

    // `alerts get <alert_id>` is a genuine positional with no flag.
    let get = alerts_subs.iter().find(|s| s["name"] == "get").unwrap();
    let id_arg = get["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "alert_id")
        .expect("alerts get should expose an 'alert_id' argument");
    assert_eq!(
        id_arg["positional"], true,
        "alerts get <alert_id> must be reported as positional"
    );
    assert!(
        id_arg.get("flag").is_none(),
        "positional args must not advertise a flag"
    );
}
