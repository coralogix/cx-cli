use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;

use super::api::{self, Entry, IndexLine, Kind, Page, QueryItem, QueryRequest, SearchMatch};
use super::get::{author_display, day};
use super::{print_raw, single_target, subject_list, subjects, Paging};
use crate::config::OutputFormat;
use crate::execution::ExecutionTarget;

const MATCH_LINES_SHOWN: usize = 3;

// The selection and paging shared by `list` and `search`.
#[derive(clap::Args, Debug, Clone)]
pub struct Filter {
    /// Only entries with this subject (`type` or `type:value`). Repeatable.
    #[arg(long = "subject", value_name = "SUBJECT", conflicts_with = "team_wide")]
    pub subjects: Vec<String>,

    /// Only team-wide entries.
    #[arg(long)]
    pub team_wide: bool,

    /// Include deleted entries.
    #[arg(long)]
    pub include_deleted: bool,

    #[command(flatten)]
    pub paging: Paging,
}

/// `list` when `patterns` is empty, otherwise `search`.
pub async fn run_query(
    targets: &[Arc<ExecutionTarget>],
    kind: Kind,
    patterns: Vec<String>,
    fixed: bool,
    filter: &Filter,
    output: OutputFormat,
) -> Result<()> {
    let target = single_target(targets)?;
    let verb = if patterns.is_empty() {
        "Listing"
    } else {
        "Searching"
    };
    let req = QueryRequest {
        patterns,
        fixed,
        subjects: subjects::for_read(&filter.subjects)?,
        team_wide: filter.team_wide,
        include_deleted: filter.include_deleted,
        limit: filter.paging.limit,
        offset: filter.paging.offset,
    };
    eprintln!("{}", format!("{verb} {}...", kind.noun()).dimmed());
    let reply = api::query_entries(&target.client, kind, &req).await?;
    match output {
        OutputFormat::Text => {
            println!("{}", render_query(&reply.data, kind));
            Ok(())
        }
        _ => print_raw(&reply.raw, output),
    }
}

pub async fn run_entry_get(
    targets: &[Arc<ExecutionTarget>],
    kind: Kind,
    id: &str,
    revision: Option<u64>,
    output: OutputFormat,
) -> Result<()> {
    let target = single_target(targets)?;
    eprintln!("{}", format!("Reading {}...", kind.singular()).dimmed());
    let reply = api::get_entry(&target.client, kind, id, revision).await?;
    match output {
        OutputFormat::Text => {
            print!("{}", render_entry(&reply.data));
            Ok(())
        }
        _ => print_raw(&reply.raw, output),
    }
}

/// `Showing A-B of TOTAL.` with the next offset when more remain; `None` when
/// the page is empty.
pub(super) fn showing_line(offset: u64, returned: u64, total: u64) -> Option<String> {
    if returned == 0 {
        return None;
    }
    let end = offset + returned;
    let more = if end < total {
        format!(" More: --offset {end}")
    } else {
        String::new()
    };
    Some(format!("Showing {}-{end} of {total}.{more}", offset + 1))
}

fn subjects_display(index: &IndexLine) -> String {
    if index.subjects.is_empty() {
        "team-wide".into()
    } else {
        subject_list(&index.subjects)
    }
}

fn match_line(m: &SearchMatch) -> String {
    let start = if m.cut_start { "…" } else { "" };
    let end = if m.cut_end { "…" } else { "" };
    format!("    {}:{}: {start}{}{end}", m.field, m.line, m.text)
}

pub fn render_query(page: &Page<QueryItem>, kind: Kind) -> String {
    let returned = page.items.len() as u64;
    let Some(showing) = showing_line(page.offset, returned, page.total) else {
        return if page.total == 0 {
            format!("No {} found.", kind.noun())
        } else {
            format!(
                "No {} at this offset; there are {}.",
                kind.noun(),
                page.total
            )
        };
    };
    let mut lines = Vec::new();
    for item in &page.items {
        let i = &item.index;
        let deleted = if i.deleted { " (deleted)" } else { "" };
        lines.push(format!(
            "{}  {}  [{}]  {}{deleted}",
            i.id,
            day(&i.updated_at),
            subjects_display(i),
            i.title
        ));
        lines.push(format!("    {}", i.summary));
        lines.extend(item.matches.iter().take(MATCH_LINES_SHOWN).map(match_line));
        let shown = item.matches.len().min(MATCH_LINES_SHOWN) as u64;
        if let Some(count) = item.match_count.filter(|&c| c > shown) {
            lines.push(format!("    ({count} matching lines)"));
        }
    }
    lines.push(String::new());
    lines.push(showing);
    lines.join("\n")
}

/// The `get <ID>` text, ending in the body as stored plus a final newline.
pub fn render_entry(e: &Entry) -> String {
    let i = &e.index;
    let mut lines = vec![
        format!("{} {}  rev {}", i.kind, i.id, i.revision),
        format!("Title:    {}", i.title),
        format!("Summary:  {}", i.summary),
        format!("Subjects: {}", subjects_display(i)),
    ];
    if let Some(source) = &e.source {
        lines.push(format!("Source:   {source}"));
    }
    lines.push(format!("Created:  {}", day(&i.created_at)));
    lines.push(format!(
        "Updated:  {} by {}",
        day(&i.updated_at),
        author_display(&i.updated_by)
    ));
    if i.deleted {
        lines.push("Deleted:  yes".into());
    }
    lines.push(String::new());
    lines.push(e.body.clone());
    let mut text = lines.join("\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const ID: &str = "9a2f6c1e-7d3b-4c2a-9f1e-0b1c2d3e4f50";

    fn item(subjects: Value, deleted: bool, matches: Option<(u64, Value)>) -> Value {
        let mut v = json!({
            "id": ID, "kind": "memory", "title": "Checkout timeouts", "summary": "Retry storms.",
            "subjects": subjects, "deleted": deleted, "revision": 2,
            "created_at": "2026-11-01T09:00:00Z", "updated_at": "2026-11-08T10:00:00Z",
            "updated_by": { "user_id": null, "name": null, "credential": "team_api_key" },
        });
        if let Some((count, m)) = matches {
            v["match_count"] = json!(count);
            v["matches"] = m;
        }
        v
    }

    fn page(items: Vec<Value>, total: u64, offset: u64) -> Page<QueryItem> {
        serde_json::from_value(
            json!({ "items": items, "total": total, "limit": 50, "offset": offset }),
        )
        .unwrap()
    }

    #[test]
    fn showing_line_variants() {
        assert_eq!(
            showing_line(0, 3, 10).unwrap(),
            "Showing 1-3 of 10. More: --offset 3"
        );
        assert_eq!(showing_line(7, 3, 10).unwrap(), "Showing 8-10 of 10.");
        assert_eq!(showing_line(0, 0, 10), None);
    }

    #[test]
    fn empty_and_past_the_end() {
        assert_eq!(
            render_query(&page(vec![], 0, 0), Kind::Memory),
            "No memories found."
        );
        assert_eq!(
            render_query(&page(vec![], 4, 10), Kind::Instruction),
            "No instructions at this offset; there are 4."
        );
    }

    #[test]
    fn team_wide_and_deleted_markers() {
        let text = render_query(&page(vec![item(json!([]), true, None)], 1, 0), Kind::Memory);
        assert_eq!(
            text,
            format!(
                "{ID}  2026-11-08  [team-wide]  Checkout timeouts (deleted)\n    Retry storms.\n\nShowing 1-1 of 1."
            )
        );
    }

    #[test]
    fn search_lines_are_capped_with_count_and_cut_markers() {
        let m = |line: u64, s: bool, e: bool| json!({ "field": "body", "line": line, "text": "pool timeout", "cut_start": s, "cut_end": e });
        let matches = json!([
            m(1, false, false),
            m(2, true, false),
            m(3, false, true),
            m(4, true, true)
        ]);
        let text = render_query(
            &page(
                vec![item(
                    json!([{ "type": "service", "value": "checkout" }]),
                    false,
                    Some((5, matches)),
                )],
                1,
                0,
            ),
            Kind::Memory,
        );
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            &lines[2..6],
            [
                "    body:1: pool timeout",
                "    body:2: …pool timeout",
                "    body:3: pool timeout…",
                "    (5 matching lines)",
            ]
        );
    }

    #[test]
    fn match_count_equal_to_shown_has_no_count_line() {
        let matches = json!([{ "field": "title", "line": 1, "text": "x", "cut_start": false, "cut_end": false }]);
        let text = render_query(
            &page(vec![item(json!([]), false, Some((1, matches)))], 1, 0),
            Kind::Memory,
        );
        assert!(!text.contains("matching lines"), "{text}");
    }

    fn entry(source: Value, subjects: Value, deleted: bool, body: &str) -> Entry {
        let mut v = item(subjects.clone(), deleted, None);
        v["updated_by"] = json!({ "user_id": "u-1", "name": "Dana", "credential": "user_token" });
        v["body"] = json!(body);
        v["source"] = source;
        v["team_wide"] = json!(subjects.as_array().unwrap().is_empty());
        v["change_note"] = Value::Null;
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn entry_header_with_source_and_subjects() {
        let e = entry(
            json!("incident review"),
            json!([{ "type": "service", "value": "checkout" }, { "type": "alert" }]),
            false,
            "line one\nline two",
        );
        assert_eq!(
            render_entry(&e),
            format!(
                "memory {ID}  rev 2\nTitle:    Checkout timeouts\nSummary:  Retry storms.\nSubjects: service:checkout alert\nSource:   incident review\nCreated:  2026-11-01\nUpdated:  2026-11-08 by Dana\n\nline one\nline two\n"
            )
        );
    }

    #[test]
    fn entry_team_wide_deleted_without_source_keeps_body() {
        let e = entry(Value::Null, json!([]), true, "body\n\n");
        assert_eq!(
            render_entry(&e),
            format!(
                "memory {ID}  rev 2\nTitle:    Checkout timeouts\nSummary:  Retry storms.\nSubjects: team-wide\nCreated:  2026-11-01\nUpdated:  2026-11-08 by Dana\nDeleted:  yes\n\nbody\n\n"
            )
        );
    }
}
