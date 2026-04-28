use anyhow::Result;
use clap::Command;
use serde_json::{json, Value};

pub fn run(cmd: Command) -> Result<()> {
    let schema = build_schema(&cmd);
    println!("{}", serde_json::to_string_pretty(&schema)?);
    Ok(())
}

pub fn build_schema(cmd: &Command) -> Value {
    let version = cmd.get_version().unwrap_or("unknown");
    json!({
        "name": cmd.get_name(),
        "version": version,
        "commands": cmd.get_subcommands()
            .filter(|sub| !sub.is_hide_set() && sub.get_name() != "help")
            .map(|sub| build_command(sub))
            .collect::<Vec<_>>()
    })
}

fn build_command(cmd: &Command) -> Value {
    let subcommands: Vec<Value> = cmd
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set() && sub.get_name() != "help")
        .map(|sub| build_command(sub))
        .collect();

    let arguments: Vec<Value> = cmd
        .get_arguments()
        .filter(|arg| arg.get_id() != "help" && arg.get_id() != "version")
        .map(|arg| {
            let mut obj = json!({
                "name": arg.get_id().as_str(),
                "required": arg.is_required_set(),
            });
            if let Some(help) = arg.get_help() {
                obj["description"] = json!(help.to_string());
            }
            if let Some(vals) = arg.get_default_values().first() {
                obj["default"] = json!(vals.to_string_lossy());
            }
            if let Some(value_names) = arg.get_value_names() {
                let type_name = value_names
                    .first()
                    .map(|v| v.to_lowercase())
                    .unwrap_or_else(|| "string".to_string());
                obj["type"] = json!(type_name);
            } else {
                obj["type"] = json!("string");
            }
            obj
        })
        .collect();

    let mut obj = json!({
        "name": cmd.get_name(),
    });
    if let Some(about) = cmd.get_about() {
        obj["description"] = json!(about.to_string());
    }
    if !subcommands.is_empty() {
        obj["subcommands"] = json!(subcommands);
    }
    if !arguments.is_empty() {
        obj["arguments"] = json!(arguments);
    }
    obj
}
