use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};

use super::api::{self, Subject, SubjectTypes};
use super::{parse_subjects, print_raw, single_target};
use crate::config::OutputFormat;
use crate::execution::ExecutionTarget;
use crate::render;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdForm {
    Uuid,
    DataspaceDataset,
    Unchecked,
}

/// A subject type that names a Coralogix entity with one ID form.
struct KnownType {
    name: &'static str,
    /// Noun with its article, for messages.
    entity: &'static str,
    id: &'static str,
    form: IdForm,
    find: &'static str,
}

const KNOWN_TYPES: &[KnownType] = &[
    KnownType {
        name: "alert",
        entity: "an alert",
        id: "its definition ID, a UUID (not the name or a version ID)",
        form: IdForm::Uuid,
        find: "cx alerts list --name <text>",
    },
    KnownType {
        name: "case",
        entity: "a case",
        id: "its UUID (not the readable CASE-123)",
        form: IdForm::Uuid,
        find: "cx cases get CASE-123",
    },
    KnownType {
        name: "dashboard",
        entity: "a dashboard",
        id: "its ID",
        form: IdForm::Unchecked,
        find: "cx dashboards search <text>",
    },
    KnownType {
        name: "dataset",
        entity: "a dataset",
        id: "dataspace/dataset, for example default/logs",
        form: IdForm::DataspaceDataset,
        find: "the source of a DataPrime query",
    },
    KnownType {
        name: "slo",
        entity: "an SLO",
        id: "its UUID",
        form: IdForm::Uuid,
        find: "cx slos list",
    },
];

fn known(subject_type: &str) -> Option<&'static KnownType> {
    KNOWN_TYPES.iter().find(|k| k.name == subject_type)
}

fn is_uuid(value: &str) -> bool {
    value.len() == 36 && uuid::Uuid::try_parse(value).is_ok()
}

fn is_dataspace_dataset(value: &str) -> bool {
    matches!(value.split_once('/'), Some((space, set)) if !space.is_empty() && !set.is_empty())
}

/// Why the value cannot be the ID of the Coralogix entity its type names, if so.
fn problem(subject: &Subject) -> Option<String> {
    let value = subject.value.as_deref()?;
    let known = known(&subject.subject_type)?;
    let fits = match known.form {
        IdForm::Uuid => is_uuid(value),
        IdForm::DataspaceDataset => is_dataspace_dataset(value),
        IdForm::Unchecked => true,
    };
    (!fits).then(|| {
        format!(
            "Subject '{}:{value}' does not look like {}: {} is identified by {}. Find it with: {}",
            subject.subject_type, known.entity, known.entity, known.id, known.find
        )
    })
}

/// Subjects for a write: a value that cannot be the entity's ID is refused, because the entry
/// would be attached to nothing a later reader looks up.
pub fn for_write(tokens: &[String]) -> Result<Vec<Subject>> {
    let subjects = parse_subjects(tokens)?;
    if let Some(message) = subjects.iter().find_map(problem) {
        bail!("{message}");
    }
    Ok(subjects)
}

/// Subjects for a read: the same check only warns, since a wrong value just finds nothing.
pub fn for_read(tokens: &[String]) -> Result<Vec<Subject>> {
    let subjects = parse_subjects(tokens)?;
    for message in subjects.iter().filter_map(problem) {
        eprintln!("{} {message}", "warning:".yellow());
    }
    Ok(subjects)
}

/// One line of the listing; `counts` is `None` for a known type missing from a cut response,
/// where zero would claim that nothing is saved under it.
struct Row {
    subject_type: String,
    counts: Option<(u64, u64)>,
    known: Option<&'static KnownType>,
}

/// The types in use, then the known types the response does not hold.
fn rows(types: &SubjectTypes) -> Vec<Row> {
    let cut = types.total > types.types.len() as u64;
    let mut rows: Vec<Row> = types
        .types
        .iter()
        .map(|t| Row {
            subject_type: t.subject_type.clone(),
            counts: Some((t.memories, t.instructions)),
            known: known(&t.subject_type),
        })
        .collect();
    for k in KNOWN_TYPES {
        if !types.types.iter().any(|t| t.subject_type == k.name) {
            rows.push(Row {
                subject_type: k.name.to_string(),
                counts: (!cut).then_some((0, 0)),
                known: Some(k),
            });
        }
    }
    rows
}

pub fn render_subject_types(types: &SubjectTypes) -> String {
    let cells = rows(types)
        .into_iter()
        .map(|r| {
            let count = |n: Option<u64>| n.map_or("?".to_string(), |n| n.to_string());
            // The leading cell is the profile column `format_table` drops.
            vec![
                String::new(),
                r.subject_type,
                count(r.counts.map(|c| c.0)),
                count(r.counts.map(|c| c.1)),
                r.known.map(|k| k.id.to_string()).unwrap_or_default(),
                r.known.map(|k| k.find.to_string()).unwrap_or_default(),
            ]
        })
        .collect();
    let table = render::format_table(
        &[
            "Type",
            "Memories",
            "Instructions",
            "Coralogix ID",
            "Find it with",
        ],
        cells,
        false,
    );
    let more = if types.total > types.types.len() as u64 {
        format!(
            "\nShowing the {} most used of {} types in use; ? marks a count that is not known.",
            types.types.len(),
            types.total
        )
    } else {
        String::new()
    };
    format!(
        "{table}\n{more}\nTypes with a Coralogix ID name one Coralogix entity; use that ID as the value.\n\
         Other types are free: reuse a type already in use before coining a new one."
    )
}

fn as_json(types: &SubjectTypes) -> Value {
    let list: Vec<Value> = rows(types)
        .into_iter()
        .map(|r| {
            json!({
                "type": r.subject_type,
                "memories": r.counts.map(|c| c.0),
                "instructions": r.counts.map(|c| c.1),
                "coralogix_id": r.known.map(|k| k.id),
                "find_with": r.known.map(|k| k.find),
            })
        })
        .collect();
    json!({ "types": list, "total": types.total })
}

pub async fn run_subjects(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    let target = single_target(targets)?;
    eprintln!("{}", "Reading subject types...".dimmed());
    let reply = api::subject_types(&target.client).await?;
    match output {
        OutputFormat::Text => {
            println!("{}", render_subject_types(&reply.data));
            Ok(())
        }
        _ => print_raw(&as_json(&reply.data), output),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::context::api::SubjectTypeCount;

    const UUID: &str = "8f3a2c1e-5b7d-4e2a-9c1f-0d6b3a4e5f60";

    fn tokens(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn write_accepts_ids_types_alone_and_free_subjects() {
        let ok = [
            format!("alert:{UUID}"),
            format!("case:{UUID}"),
            format!("slo:{UUID}"),
            "dataset:default/logs".into(),
            "dataset:default/enrichments/ip_lookup".into(),
            "dashboard:anything".into(),
            "alert".into(),
            "dataset".into(),
            "service:checkout".into(),
            "host:i-0abc1234def567890".into(),
            "ip:2001:db8::".into(),
        ];
        assert_eq!(for_write(&ok).unwrap().len(), ok.len());
    }

    #[test]
    fn write_refuses_values_that_cannot_be_the_id() {
        for bad in [
            "alert:checkout-error-rate",
            "alert:8f3a2c1e5b7d4e2a9c1f0d6b3a4e5f60",
            "case:CASE-123",
            "slo:latency",
            "dataset:logs",
            "dataset:/logs",
            "dataset:default/",
        ] {
            let err = for_write(&tokens(&[bad])).unwrap_err().to_string();
            assert!(err.contains(&format!("'{bad}'")), "{err}");
            assert!(err.contains("Find it with:"), "{err}");
        }
        let err = for_write(&tokens(&["case:CASE-123"]))
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            "Subject 'case:CASE-123' does not look like a case: a case is identified by its UUID \
             (not the readable CASE-123). Find it with: cx cases get CASE-123"
        );
    }

    #[test]
    fn read_keeps_a_doubtful_subject() {
        let read = for_read(&tokens(&["alert:by-name", "host:i-1"])).unwrap();
        assert_eq!(read.len(), 2);
    }

    fn count(t: &str, memories: u64, instructions: u64) -> SubjectTypeCount {
        SubjectTypeCount {
            subject_type: t.into(),
            memories,
            instructions,
        }
    }

    #[test]
    fn lists_types_in_use_then_unused_known_types() {
        let types = SubjectTypes {
            types: vec![count("host", 4, 0), count("alert", 2, 1)],
            total: 2,
        };
        let names: Vec<String> = rows(&types).into_iter().map(|r| r.subject_type).collect();
        assert_eq!(
            names,
            ["host", "alert", "case", "dashboard", "dataset", "slo"]
        );
        let text = render_subject_types(&types);
        assert!(text.contains("cx alerts list --name <text>"), "{text}");
        assert!(!text.contains("most used of"), "{text}");
        let json = as_json(&types);
        assert_eq!(json["total"], 2);
        assert_eq!(json["types"][0]["coralogix_id"], Value::Null);
        assert_eq!(
            json["types"][1]["find_with"],
            "cx alerts list --name <text>"
        );
    }

    #[test]
    fn says_when_the_list_is_cut() {
        let types = SubjectTypes {
            types: vec![count("host", 4, 0)],
            total: 700,
        };
        let text = render_subject_types(&types);
        assert!(
            text.contains("Showing the 1 most used of 700 types in use"),
            "{text}"
        );
        assert!(text.contains("| slo       | ?"), "{text}");
        let slo = &as_json(&types)["types"][5];
        assert_eq!(slo["type"], "slo");
        assert_eq!(slo["memories"], Value::Null);
    }
}
