use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;

use super::api::{self, HistoryQuery, Kind, Page, RevisionItem, TeamRevisionItem};
use super::entries::showing_line;
use super::get::author_display;
use super::{print_raw, single_target, Paging};
use crate::config::OutputFormat;
use crate::execution::ExecutionTarget;
use crate::render;

// `since` is what the user typed; it is parsed when the query is built.
#[derive(clap::Args, Debug, Clone)]
pub struct HistoryFilter {
    /// Author user ID or exact author name.
    #[arg(long)]
    pub author: Option<String>,

    /// Case-insensitive substring of the entry's source.
    #[arg(long)]
    pub source: Option<String>,

    /// Only changes to this kind of entry.
    #[arg(long)]
    pub kind: Option<Kind>,

    /// Only changes at or after this time (ISO 8601 or relative, e.g. now-7d).
    #[arg(long)]
    pub since: Option<String>,

    #[command(flatten)]
    pub paging: Paging,
}

pub async fn run_team_history(
    targets: &[Arc<ExecutionTarget>],
    filter: &HistoryFilter,
    output: OutputFormat,
) -> Result<()> {
    let target = single_target(targets)?;
    let query = HistoryQuery {
        author: filter.author.clone(),
        source: filter.source.clone(),
        kind: filter.kind,
        since: filter
            .since
            .as_deref()
            .map(crate::time::parse_timestamp)
            .transpose()?,
        limit: filter.paging.limit,
        offset: filter.paging.offset,
    };
    eprintln!("{}", "Reading history...".dimmed());
    let reply = api::team_history(&target.client, &query).await?;
    match output {
        OutputFormat::Text => {
            println!("{}", render_team_history(&reply.data));
            Ok(())
        }
        _ => print_raw(&reply.raw, output),
    }
}

pub async fn run_entry_history(
    targets: &[Arc<ExecutionTarget>],
    kind: Kind,
    id: &str,
    limit: u32,
    offset: u64,
    output: OutputFormat,
) -> Result<()> {
    let target = single_target(targets)?;
    eprintln!(
        "{}",
        format!("Reading {} history...", kind.singular()).dimmed()
    );
    let reply = api::entry_revisions(&target.client, kind, id, limit, offset).await?;
    match output {
        OutputFormat::Text => {
            println!("{}", render_entry_history(&reply.data));
            Ok(())
        }
        _ => print_raw(&reply.raw, output),
    }
}

fn second(timestamp: &str) -> String {
    timestamp
        .get(..19)
        .unwrap_or(timestamp)
        .replacen('T', " ", 1)
}

fn operation(r: &RevisionItem) -> String {
    match (r.operation.as_str(), r.reverted_to) {
        ("revert", Some(to)) => format!("revert to {to}"),
        (op, _) => op.to_string(),
    }
}

/// Time, revision, operation, author, title, change note, with `extra` after the time.
fn row(r: &RevisionItem, extra: &[&str]) -> Vec<String> {
    // The leading cell is the profile column `format_table` drops.
    let mut cells = vec![String::new(), second(&r.created_at)];
    cells.extend(extra.iter().map(|s| s.to_string()));
    cells.extend([
        r.revision.to_string(),
        operation(r),
        author_display(&r.author).to_string(),
        r.title.clone(),
        r.change_note.clone().unwrap_or_default(),
    ]);
    cells
}

fn render_page<T>(page: &Page<T>, headers: &[&str], row_of: impl Fn(&T) -> Vec<String>) -> String {
    let returned = page.items.len() as u64;
    let Some(showing) = showing_line(page.offset, returned, page.total) else {
        return if page.total == 0 {
            "No changes found.".into()
        } else {
            format!("No changes at this offset; there are {}.", page.total)
        };
    };
    let rows = page.items.iter().map(row_of).collect();
    format!(
        "{}\n\n{showing}",
        render::format_table(headers, rows, false)
    )
}

pub fn render_entry_history(page: &Page<RevisionItem>) -> String {
    render_page(
        page,
        &[
            "Time (UTC)",
            "Rev",
            "Operation",
            "Author",
            "Title",
            "Change note",
        ],
        |r| row(r, &[]),
    )
}

pub fn render_team_history(page: &Page<TeamRevisionItem>) -> String {
    render_page(
        page,
        &[
            "Time (UTC)",
            "ID",
            "Kind",
            "Rev",
            "Operation",
            "Author",
            "Title",
            "Change note",
        ],
        |t| row(&t.revision, &[&t.id, &t.kind]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const ID: &str = "9a2f6c1e-7d3b-4c2a-9f1e-0b1c2d3e4f50";

    fn revision(rev: u64, op: &str, reverted_to: Option<u64>, name: Option<&str>) -> Value {
        json!({
            "revision": rev, "operation": op, "reverted_to": reverted_to,
            "change_note": if op == "update" { json!("pool size") } else { Value::Null },
            "author": { "user_id": null, "name": name, "credential": "team_api_key" },
            "client": "cx", "created_at": "2026-11-08T10:00:05.123456Z",
            "title": "Checkout timeouts", "deleted": op == "delete",
        })
    }

    fn page<T: serde::de::DeserializeOwned>(items: Vec<Value>, total: u64, offset: u64) -> Page<T> {
        serde_json::from_value(
            json!({ "items": items, "total": total, "limit": 50, "offset": offset }),
        )
        .unwrap()
    }

    #[test]
    fn entry_rows_show_operations_authors_and_seconds() {
        let text = render_entry_history(&page(
            vec![
                revision(4, "revert", Some(2), Some("Dana")),
                revision(3, "delete", None, None),
                revision(2, "update", None, Some("Lee")),
            ],
            5,
            0,
        ));
        assert!(text.contains("2026-11-08 10:00:05"), "{text}");
        assert!(!text.contains(".123456"), "{text}");
        assert!(text.contains("revert to 2"), "{text}");
        assert!(text.contains("team_api_key"), "{text}");
        assert!(text.contains("pool size"), "{text}");
        assert!(text.contains("| delete"), "{text}");
        assert!(
            text.ends_with("\n\nShowing 1-3 of 5. More: --offset 3"),
            "{text}"
        );
    }

    #[test]
    fn team_rows_add_id_and_kind_after_time() {
        let mut item = revision(1, "create", None, Some("Dana"));
        item["id"] = json!(ID);
        item["kind"] = json!("instruction");
        item["current_revision"] = json!(3);
        let text = render_team_history(&page(vec![item], 1, 0));
        let header = text.lines().nth(1).unwrap();
        let time = header.find("Time (UTC)").unwrap();
        let id = header.find("ID").unwrap();
        let kind = header.find("Kind").unwrap();
        let rev = header.find("Rev").unwrap();
        assert!(time < id && id < kind && kind < rev, "{header}");
        assert!(text.contains(ID) && text.contains("instruction"), "{text}");
        assert!(text.ends_with("\n\nShowing 1-1 of 1."), "{text}");
    }

    #[test]
    fn empty_and_past_the_end() {
        assert_eq!(
            render_entry_history(&page(vec![], 0, 0)),
            "No changes found."
        );
        assert_eq!(
            render_team_history(&page(vec![], 7, 50)),
            "No changes at this offset; there are 7."
        );
    }
}
