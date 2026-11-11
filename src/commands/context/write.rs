use std::io::Read;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use colored::Colorize;

use super::api::{self, CreateRequest, Entry, EntryContent, Kind, Receipt, Subject, UpdateRequest};
use super::{parse_subjects, print_raw, single_target, subject_list};
use crate::config::OutputFormat;
use crate::execution::ExecutionTarget;
use crate::safety::confirm_destructive;

/// How a write was confirmed: the global `--yes` and agent detection.
#[derive(Debug, Clone, Copy)]
pub struct Confirmation {
    pub yes: bool,
    pub agent_mode: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct CreateArgs {
    /// One line saying what the entry is about.
    #[arg(long)]
    pub title: String,

    /// One line saying when the entry is useful.
    #[arg(long)]
    pub summary: String,

    /// Subject (`type` or `type:value`). Repeatable.
    #[arg(long = "subject", value_name = "SUBJECT", conflicts_with = "team_wide")]
    pub subjects: Vec<String>,

    /// Apply to the whole team instead of subjects.
    #[arg(long)]
    pub team_wide: bool,

    /// File holding the body text (`-` for stdin).
    #[arg(long, value_name = "PATH")]
    pub body_file: String,

    /// Where the content came from (a case, an alert event, a link).
    #[arg(long)]
    pub source: Option<String>,

    /// Entry ID to use instead of a new UUID; pass the same ID to retry a failed create.
    #[arg(long, value_name = "UUID")]
    pub id: Option<String>,
}

fn article(kind: Kind) -> &'static str {
    match kind {
        Kind::Memory => "a",
        Kind::Instruction => "an",
    }
}

/// The body text from a file, or stdin for `-`.
fn read_body(path: &str) -> Result<String> {
    if path == "-" {
        let mut body = String::new();
        std::io::stdin()
            .read_to_string(&mut body)
            .context("Failed to read the body from stdin")?;
        return Ok(body);
    }
    std::fs::read_to_string(path).with_context(|| format!("Failed to read body file {path}"))
}

pub async fn run_create(
    targets: &[Arc<ExecutionTarget>],
    kind: Kind,
    args: CreateArgs,
    confirmation: Confirmation,
    output: OutputFormat,
) -> Result<()> {
    let target = single_target(targets)?;
    let subjects = parse_subjects(&args.subjects)?;
    confirm_destructive(
        &format!(
            "Create {} {} '{}'?",
            article(kind),
            kind.singular(),
            args.title
        ),
        confirmation.yes,
        confirmation.agent_mode,
    )?;
    let body = read_body(&args.body_file)?;
    let id = args.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let req = CreateRequest {
        id,
        content: EntryContent {
            title: args.title,
            summary: args.summary,
            body,
            subjects,
            team_wide: args.team_wide,
            source: args.source,
        },
    };
    eprintln!("{}", format!("Saving {}...", kind.singular()).dimmed());
    let reply = api::create_entry(&target.client, kind, &req)
        .await
        .map_err(|e| anyhow!("{e}\nTo retry without saving twice, pass --id {}.", req.id))?;
    match output {
        OutputFormat::Text => {
            println!("{}", render_create(&reply.data, kind, reply.status));
            Ok(())
        }
        _ => print_raw(&reply.raw, output),
    }
}

#[derive(clap::Args, Debug, Clone)]
pub struct UpdateArgs {
    /// Revision the change is based on.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub expected_revision: u64,

    /// New title.
    #[arg(long)]
    pub title: Option<String>,

    /// New summary.
    #[arg(long)]
    pub summary: Option<String>,

    /// Replace all subjects (`type` or `type:value`). Repeatable.
    #[arg(long = "subject", value_name = "SUBJECT", conflicts_with = "team_wide")]
    pub subjects: Vec<String>,

    /// Remove all subjects and apply to the whole team.
    #[arg(long)]
    pub team_wide: bool,

    /// File holding the new body text (`-` for stdin).
    #[arg(long, value_name = "PATH")]
    pub body_file: Option<String>,

    /// New source.
    #[arg(long, conflicts_with = "no_source")]
    pub source: Option<String>,

    /// Clear the source.
    #[arg(long)]
    pub no_source: bool,

    /// Why the entry changed.
    #[arg(long)]
    pub change_note: Option<String>,
}

/// The parsed command-line side of an update.
#[derive(Debug, Clone, Default)]
struct Edits {
    title: Option<String>,
    summary: Option<String>,
    body: Option<String>,
    subjects: Vec<Subject>,
    team_wide: bool,
    source: Option<String>,
    no_source: bool,
}

/// The full replacement: what the command line gives, otherwise the entry's own
/// title, summary, body, source and subject set.
fn merge(current: &Entry, edits: Edits) -> EntryContent {
    let (subjects, team_wide) = if !edits.subjects.is_empty() {
        (edits.subjects, false)
    } else if edits.team_wide {
        (Vec::new(), true)
    } else {
        (current.index.subjects.clone(), current.team_wide)
    };
    let source = if edits.no_source {
        None
    } else {
        edits.source.or_else(|| current.source.clone())
    };
    EntryContent {
        title: edits.title.unwrap_or_else(|| current.index.title.clone()),
        summary: edits
            .summary
            .unwrap_or_else(|| current.index.summary.clone()),
        body: edits.body.unwrap_or_else(|| current.body.clone()),
        subjects,
        team_wide,
        source,
    }
}

/// Refuses to write over an entry that is deleted or not at the expected revision.
fn check_current(current: &Entry, kind: Kind, id: &str, expected: u64) -> Result<()> {
    if current.index.deleted {
        bail!(api::deleted_message(kind, id));
    }
    if current.index.revision != expected {
        bail!(api::revision_conflict_message(
            kind,
            id,
            current.index.revision,
            expected
        ));
    }
    Ok(())
}

pub async fn run_update(
    targets: &[Arc<ExecutionTarget>],
    kind: Kind,
    id: &str,
    args: UpdateArgs,
    confirmation: Confirmation,
    output: OutputFormat,
) -> Result<()> {
    let target = single_target(targets)?;
    let subjects = parse_subjects(&args.subjects)?;
    confirm_destructive(
        &format!("Update {} {id}?", kind.singular()),
        confirmation.yes,
        confirmation.agent_mode,
    )?;
    let body = args.body_file.as_deref().map(read_body).transpose()?;
    eprintln!("{}", format!("Reading {}...", kind.singular()).dimmed());
    let current = api::get_entry(&target.client, kind, id, None).await?.data;
    check_current(&current, kind, id, args.expected_revision)?;
    let edits = Edits {
        title: args.title,
        summary: args.summary,
        body,
        subjects,
        team_wide: args.team_wide,
        source: args.source,
        no_source: args.no_source,
    };
    let req = UpdateRequest {
        content: merge(&current, edits),
        expected_revision: args.expected_revision,
        change_note: args.change_note,
    };
    eprintln!("{}", format!("Saving {}...", kind.singular()).dimmed());
    let reply = api::update_entry(&target.client, kind, id, &req).await?;
    match output {
        OutputFormat::Text => {
            let i = &reply.data.entry.index;
            let first = format!(
                "Updated {} {}, now at revision {}.",
                kind.singular(),
                i.id,
                i.revision
            );
            println!("{}", render_receipt(first, &reply.data, kind));
            Ok(())
        }
        _ => print_raw(&reply.raw, output),
    }
}

pub async fn run_delete(
    targets: &[Arc<ExecutionTarget>],
    kind: Kind,
    id: &str,
    expected_revision: u64,
    confirmation: Confirmation,
    output: OutputFormat,
) -> Result<()> {
    let target = single_target(targets)?;
    confirm_destructive(
        &format!("Delete {} {id}?", kind.singular()),
        confirmation.yes,
        confirmation.agent_mode,
    )?;
    eprintln!("{}", format!("Deleting {}...", kind.singular()).dimmed());
    let reply = api::delete_entry(&target.client, kind, id, expected_revision).await?;
    match output {
        OutputFormat::Text => {
            println!("{}", render_delete(&reply.data, kind));
            Ok(())
        }
        _ => print_raw(&reply.raw, output),
    }
}

pub fn render_delete(entry: &Entry, kind: Kind) -> String {
    let (id, n) = (&entry.index.id, entry.index.revision);
    format!(
        "Deleted {k} {id}, now at revision {n}.\nRestore with: cx context {noun} revert {id} --to-revision {prev} --expected-revision {n}",
        k = kind.singular(),
        noun = kind.noun(),
        prev = n.saturating_sub(1),
    )
}

pub async fn run_revert(
    targets: &[Arc<ExecutionTarget>],
    kind: Kind,
    id: &str,
    to_revision: u64,
    expected_revision: u64,
    confirmation: Confirmation,
    output: OutputFormat,
) -> Result<()> {
    let target = single_target(targets)?;
    confirm_destructive(
        &format!("Revert {} {id} to revision {to_revision}?", kind.singular()),
        confirmation.yes,
        confirmation.agent_mode,
    )?;
    eprintln!("{}", format!("Reverting {}...", kind.singular()).dimmed());
    let reply = api::revert_entry(&target.client, kind, id, to_revision, expected_revision).await?;
    match output {
        OutputFormat::Text => {
            println!("{}", render_revert(&reply.data, kind, to_revision));
            Ok(())
        }
        _ => print_raw(&reply.raw, output),
    }
}

pub fn render_revert(receipt: &Receipt, kind: Kind, to_revision: u64) -> String {
    let i = &receipt.entry.index;
    let first = format!(
        "Reverted {} {} to revision {to_revision}, now at revision {}.",
        kind.singular(),
        i.id,
        i.revision
    );
    render_receipt(first, receipt, kind)
}

pub fn render_create(receipt: &Receipt, kind: Kind, status: u16) -> String {
    let i = &receipt.entry.index;
    let first = if status == 201 {
        format!(
            "Saved {} {}, revision {}.",
            kind.singular(),
            i.id,
            i.revision
        )
    } else {
        format!(
            "Already saved: {} {}, now at revision {}.",
            kind.singular(),
            i.id,
            i.revision
        )
    };
    render_receipt(first, receipt, kind)
}

/// The first line, then the subjects and found-by lines.
pub fn render_receipt(first: String, receipt: &Receipt, kind: Kind) -> String {
    let found = &receipt.found_by;
    let subjects = subject_list(&found.context_for);
    let subjects_line = if found.context_for.is_empty() {
        "Subjects: none (team-wide)".to_string()
    } else {
        format!("Subjects: {subjects}")
    };
    let found_by = if receipt.entry.index.deleted {
        "The entry is deleted; it is not returned by reads.".to_string()
    } else {
        match (kind, found.team_wide) {
            (Kind::Memory, false) => {
                format!("Found by: cx context get {subjects} (and by search)")
            }
            (Kind::Memory, true) => "Found by: search, list --team-wide, and cx context get without subjects; a read for subjects only counts it.".to_string(),
            (Kind::Instruction, false) => format!("Returned in full by: cx context get {subjects}"),
            (Kind::Instruction, true) => "Returned in full by every cx context get.".to_string(),
        }
    };
    [first, subjects_line, found_by].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const ID: &str = "9a2f6c1e-7d3b-4c2a-9f1e-0b1c2d3e4f50";

    fn receipt(kind: &str, subjects: Value, deleted: bool, revision: u64) -> Receipt {
        let team_wide = subjects.as_array().unwrap().is_empty();
        serde_json::from_value(json!({
            "entry": {
                "id": ID, "kind": kind, "title": "T", "summary": "S", "body": "B",
                "subjects": subjects, "team_wide": team_wide, "source": null,
                "change_note": null, "deleted": deleted, "revision": revision,
                "created_at": "2026-11-01T09:00:00Z", "updated_at": "2026-11-08T10:00:00Z",
                "updated_by": { "user_id": null, "name": null, "credential": "team_api_key" },
            },
            "found_by": { "context_for": subjects, "team_wide": team_wide, "returned_in_full": kind == "instruction" },
        }))
        .unwrap()
    }

    fn with_subjects() -> Value {
        json!([{ "type": "service", "value": "checkout" }, { "type": "host", "value": "web 01" }])
    }

    #[test]
    fn created_memory_with_subjects() {
        assert_eq!(
            render_create(&receipt("memory", with_subjects(), false, 1), Kind::Memory, 201),
            format!(
                "Saved memory {ID}, revision 1.\nSubjects: service:checkout 'host:web 01'\nFound by: cx context get service:checkout 'host:web 01' (and by search)"
            )
        );
    }

    #[test]
    fn already_saved_team_wide_memory() {
        assert_eq!(
            render_create(&receipt("memory", json!([]), false, 3), Kind::Memory, 200),
            format!(
                "Already saved: memory {ID}, now at revision 3.\nSubjects: none (team-wide)\nFound by: search, list --team-wide, and cx context get without subjects; a read for subjects only counts it."
            )
        );
    }

    #[test]
    fn instruction_found_by_lines() {
        let text = render_create(
            &receipt("instruction", with_subjects(), false, 1),
            Kind::Instruction,
            201,
        );
        assert!(text.starts_with(&format!("Saved instruction {ID}, revision 1.\n")));
        assert!(
            text.ends_with("\nReturned in full by: cx context get service:checkout 'host:web 01'")
        );
        let text = render_create(
            &receipt("instruction", json!([]), false, 1),
            Kind::Instruction,
            201,
        );
        assert!(text.ends_with("\nReturned in full by every cx context get."));
    }

    #[test]
    fn deleted_entry_replaces_found_by() {
        assert_eq!(
            render_create(&receipt("memory", with_subjects(), true, 2), Kind::Memory, 200),
            format!(
                "Already saved: memory {ID}, now at revision 2.\nSubjects: service:checkout 'host:web 01'\nThe entry is deleted; it is not returned by reads."
            )
        );
    }

    fn current() -> Entry {
        let mut entry = receipt("memory", with_subjects(), false, 4).entry;
        entry.source = Some("case 12".into());
        entry
    }

    fn subjects_of(value: Value) -> Vec<Subject> {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn merge_without_edits_keeps_everything() {
        let merged = merge(&current(), Edits::default());
        assert_eq!(
            serde_json::to_value(&merged).unwrap(),
            json!({ "title": "T", "summary": "S", "body": "B", "subjects": with_subjects(), "source": "case 12" })
        );
    }

    #[test]
    fn merge_replaces_each_field_alone() {
        let edits = |f: fn(&mut Edits)| {
            let mut e = Edits::default();
            f(&mut e);
            merge(&current(), e)
        };
        let m = edits(|e| e.title = Some("T2".into()));
        assert_eq!((m.title.as_str(), m.summary.as_str()), ("T2", "S"));
        let m = edits(|e| e.summary = Some("S2".into()));
        assert_eq!((m.title.as_str(), m.summary.as_str()), ("T", "S2"));
        let m = edits(|e| e.body = Some("B2".into()));
        assert_eq!((m.body.as_str(), m.title.as_str()), ("B2", "T"));
        let m = edits(|e| e.source = Some("alert 9".into()));
        assert_eq!(m.source.as_deref(), Some("alert 9"));
        assert_eq!(m.subjects, subjects_of(with_subjects()));
    }

    #[test]
    fn merge_subjects_replace_the_set_and_end_team_wide() {
        let mut entry = current();
        entry.index.subjects.clear();
        entry.team_wide = true;
        let only = json!([{ "type": "alert", "value": "8f3a" }]);
        let merged = merge(
            &entry,
            Edits {
                subjects: subjects_of(only.clone()),
                ..Edits::default()
            },
        );
        assert_eq!(merged.subjects, subjects_of(only));
        assert!(!merged.team_wide);
    }

    #[test]
    fn merge_subjects_replace_existing_subjects() {
        let only = json!([{ "type": "alert", "value": "8f3a" }]);
        let merged = merge(
            &current(),
            Edits {
                subjects: subjects_of(only.clone()),
                ..Edits::default()
            },
        );
        assert_eq!(merged.subjects, subjects_of(only));
    }

    #[test]
    fn merge_team_wide_removes_subjects() {
        let merged = merge(
            &current(),
            Edits {
                team_wide: true,
                ..Edits::default()
            },
        );
        assert!(merged.subjects.is_empty());
        assert!(merged.team_wide);
        assert_eq!(serde_json::to_value(&merged).unwrap()["team_wide"], true);
    }

    #[test]
    fn merge_no_source_clears_it() {
        let merged = merge(
            &current(),
            Edits {
                no_source: true,
                ..Edits::default()
            },
        );
        assert_eq!(merged.source, None);
        assert!(serde_json::to_value(&merged)
            .unwrap()
            .get("source")
            .is_none());
    }

    #[test]
    fn omitted_change_note_is_not_sent() {
        let mut entry = current();
        entry.change_note = Some("old note".into());
        let req = UpdateRequest {
            content: merge(&entry, Edits::default()),
            expected_revision: 4,
            change_note: None,
        };
        let body = serde_json::to_value(&req).unwrap();
        assert!(body.get("change_note").is_none(), "{body}");
        assert_eq!(body["expected_revision"], 4);
    }

    #[test]
    fn check_current_refuses_deleted_and_stale_entries() {
        assert!(check_current(&current(), Kind::Memory, ID, 4).is_ok());
        assert_eq!(
            check_current(&current(), Kind::Memory, ID, 3)
                .unwrap_err()
                .to_string(),
            api::revision_conflict_message(Kind::Memory, ID, 4, 3)
        );
        let mut deleted = current();
        deleted.index.deleted = true;
        assert_eq!(
            check_current(&deleted, Kind::Instruction, ID, 4)
                .unwrap_err()
                .to_string(),
            api::deleted_message(Kind::Instruction, ID)
        );
    }

    #[test]
    fn delete_receipt_names_the_restore() {
        let entry = receipt("instruction", with_subjects(), true, 5).entry;
        assert_eq!(
            render_delete(&entry, Kind::Instruction),
            format!(
                "Deleted instruction {ID}, now at revision 5.\nRestore with: cx context instructions revert {ID} --to-revision 4 --expected-revision 5"
            )
        );
    }

    #[test]
    fn revert_receipts() {
        assert_eq!(
            render_revert(&receipt("memory", json!([]), false, 6), Kind::Memory, 2),
            format!(
                "Reverted memory {ID} to revision 2, now at revision 6.\nSubjects: none (team-wide)\nFound by: search, list --team-wide, and cx context get without subjects; a read for subjects only counts it."
            )
        );
        let text = render_revert(
            &receipt("memory", with_subjects(), true, 7),
            Kind::Memory,
            5,
        );
        assert!(
            text.ends_with("\nThe entry is deleted; it is not returned by reads."),
            "{text}"
        );
    }

    #[test]
    fn missing_body_file_is_named() {
        let err = read_body("/nonexistent/body.md").unwrap_err().to_string();
        assert!(err.contains("/nonexistent/body.md"), "{err}");
    }
}
