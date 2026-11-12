pub mod api;
mod entries;
mod get;
mod history;
mod subjects;
mod write;

use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

use crate::config::OutputFormat;
use crate::execution::ExecutionTarget;
use crate::render;
use api::Subject;
pub use entries::{run_entry_get, run_query, Filter};
pub use get::run_get;
pub use history::{run_entry_history, run_team_history, HistoryFilter};
pub use subjects::run_subjects;
pub use write::{
    run_create, run_delete, run_revert, run_update, Confirmation, CreateArgs, UpdateArgs,
};

#[derive(clap::Args, Debug, Clone, Copy)]
pub struct Paging {
    /// Maximum number of results (1-200).
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=200))]
    pub limit: u32,

    /// Number of results to skip.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u64))]
    pub offset: u64,
}

pub const MULTI_PROFILE_MESSAGE: &str =
    "cx context does not support multi-profile. Use a single profile with -p <profile>.";

/// Every runner calls this first, before reading files or sending anything.
pub fn single_target(targets: &[Arc<ExecutionTarget>]) -> Result<&Arc<ExecutionTarget>> {
    let [target] = targets else {
        bail!(MULTI_PROFILE_MESSAGE)
    };
    Ok(target)
}

/// Prints a service response as JSON or toon.
fn print_raw(value: &Value, output: OutputFormat) -> Result<()> {
    match output {
        OutputFormat::Toon => println!(
            "{}",
            toon_format::encode_default(value).map_err(|e| anyhow!("TOON encoding failed: {e}"))?
        ),
        _ => render::render_json_auto(std::slice::from_ref(value))?,
    }
    Ok(())
}

/// `type` or `type:value`, split on the first colon.
pub fn parse_subject(token: &str) -> Result<Subject> {
    match token.split_once(':') {
        None => Ok(Subject {
            subject_type: token.to_string(),
            value: None,
        }),
        Some((subject_type, "")) => bail!(
            "Invalid subject '{token}': the value after ':' is empty. Use '{subject_type}' for the whole type."
        ),
        Some((subject_type, value)) => Ok(Subject {
            subject_type: subject_type.to_string(),
            value: Some(value.to_string()),
        }),
    }
}

pub fn parse_subjects(tokens: &[String]) -> Result<Vec<Subject>> {
    tokens.iter().map(|t| parse_subject(t)).collect()
}

/// Quotes `s` for a POSIX shell when it has anything outside `[A-Za-z0-9_.:/@-]`.
pub fn shell_quote(s: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "_.:/@-".contains(c);
    if !s.is_empty() && s.chars().all(plain) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// The subject as a token that can be pasted back as an argument.
pub fn subject_token(subject: &Subject) -> String {
    match &subject.value {
        Some(value) => shell_quote(&format!("{}:{value}", subject.subject_type)),
        None => shell_quote(&subject.subject_type),
    }
}

pub fn subject_list(subjects: &[Subject]) -> String {
    subjects
        .iter()
        .map(subject_token)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject(t: &str, v: Option<&str>) -> Subject {
        Subject {
            subject_type: t.into(),
            value: v.map(String::from),
        }
    }

    #[test]
    fn parses_on_first_colon() {
        assert_eq!(
            parse_subject("ip:2001:db8::").unwrap(),
            subject("ip", Some("2001:db8::"))
        );
        assert_eq!(
            parse_subject("service:checkout").unwrap(),
            subject("service", Some("checkout"))
        );
        assert_eq!(parse_subject("alert").unwrap(), subject("alert", None));
    }

    #[test]
    fn rejects_empty_value() {
        let err = parse_subject("alert:").unwrap_err().to_string();
        assert!(err.contains("'alert:'"), "{err}");
    }

    #[test]
    fn leaves_plain_tokens_unquoted() {
        assert_eq!(
            subject_token(&subject("ip", Some("2001:db8::"))),
            "ip:2001:db8::"
        );
        assert_eq!(subject_token(&subject("alert", None)), "alert");
        assert_eq!(
            subject_token(&subject("url", Some("https://x.io/a@b_c.d-e"))),
            "url:https://x.io/a@b_c.d-e"
        );
    }

    #[test]
    fn quotes_tokens_with_other_characters() {
        assert_eq!(subject_token(&subject("host", Some("a b"))), "'host:a b'");
        assert_eq!(
            subject_token(&subject("dataset", Some("it's"))),
            r"'dataset:it'\''s'"
        );
        assert_eq!(shell_quote(""), "''");
        assert_eq!(
            subject_list(&[subject("service", Some("a,b")), subject("alert", None)]),
            "'service:a,b' alert"
        );
    }

    #[test]
    fn quoted_tokens_round_trip_through_a_shell() {
        let subjects = [
            subject("ip", Some("2001:db8::")),
            subject("host", Some("web 01")),
            subject("service", Some("a,b")),
            subject("dataset", Some("it's")),
            subject("path", Some(r"C:\logs\app")),
            subject("tag", Some(r#"x"$y`z'\'"#)),
            subject("alert", None),
        ];
        let script = format!("printf '%s\\n' {}", subject_list(&subjects));
        let out = std::process::Command::new("sh")
            .args(["-c", &script])
            .output()
            .unwrap();
        let printed = String::from_utf8(out.stdout).unwrap();
        let parsed: Vec<Subject> = printed.lines().map(|l| parse_subject(l).unwrap()).collect();
        assert_eq!(parsed, subjects);
    }

    fn target(name: &str) -> Arc<ExecutionTarget> {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        let cfg = crate::config::ResolvedConfig {
            profile_name: name.into(),
            api_key: "k".into(),
            auth_kind: crate::config::AuthKind::ApiKey,
            endpoint: "http://127.0.0.1:9".into(),
            default_tier: crate::Tier::Archive,
            console_url: None,
            credentials_overridden: false,
        };
        Arc::new(ExecutionTarget::new(cfg).unwrap())
    }

    #[test]
    fn single_target_refuses_more_than_one() {
        let one = [target("a")];
        assert_eq!(single_target(&one).unwrap().profile_name, "a");
        let two = [target("a"), target("b")];
        assert_eq!(
            single_target(&two).err().unwrap().to_string(),
            MULTI_PROFILE_MESSAGE
        );
    }
}
