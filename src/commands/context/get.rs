use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;

use super::api::{self, Author, ContextRequest, ContextResponse};
use super::{print_raw, single_target, subject_list, subjects};
use crate::config::OutputFormat;
use crate::execution::ExecutionTarget;

const GUIDANCE_SUBJECTS: &str =
    "This lookup matches subjects exactly. Read again with subjects you discover along the way,";
const GUIDANCE_NO_SUBJECTS: &str =
    "No subjects given: showing team-wide instructions and the most recent memories.";
const GUIDANCE_SEARCH: &str =
    "or search by symptom, error or component words: cx context memories search <PATTERN>";
const GUIDANCE_ABSENCE: &str = "No match does not mean nothing relevant exists.";

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    subjects: &[String],
    limit: u32,
    offset: u64,
    output: OutputFormat,
) -> Result<()> {
    let target = single_target(targets)?;
    let req = ContextRequest {
        subjects: subjects::for_read(subjects)?,
        limit,
        offset,
    };
    eprintln!("{}", "Reading context...".dimmed());
    let reply = api::read_context(&target.client, &req).await?;
    let with_subjects = !req.subjects.is_empty();
    let hints = footer(&reply.data, with_subjects);
    match output {
        OutputFormat::Text => {
            println!("{}", render_text(&reply.data, with_subjects));
            Ok(())
        }
        _ => print_raw(&with_hints(reply.raw, hints), output),
    }
}

fn with_hints(mut raw: Value, hints: Vec<String>) -> Value {
    if let Value::Object(map) = &mut raw {
        map.insert("hints".into(), hints.into());
    }
    raw
}

pub(super) fn author_display(author: &Author) -> &str {
    author.name.as_deref().unwrap_or(&author.credential)
}

pub(super) fn day(timestamp: &str) -> &str {
    timestamp.get(..10).unwrap_or(timestamp)
}

fn not_shown(resp: &ContextResponse, with_subjects: bool) -> Option<String> {
    let page = &resp.memories;
    let shown = page.offset + page.items.len() as u64;
    let mut parts = Vec::new();
    let rest = page.total.saturating_sub(shown);
    if rest > 0 {
        let what = if with_subjects {
            "more memories for these subjects"
        } else {
            "more recent memories"
        };
        parts.push(format!("{rest} {what} (--offset {shown})"));
    }
    if with_subjects && resp.team_wide_memories > 0 {
        parts.push(format!(
            "{} team-wide memories (cx context memories list --team-wide)",
            resp.team_wide_memories
        ));
    }
    (!parts.is_empty()).then(|| format!("Not shown: {}.", parts.join("; ")))
}

/// The text footer, also returned as `hints` in JSON and toon.
pub fn footer(resp: &ContextResponse, with_subjects: bool) -> Vec<String> {
    let first = if with_subjects {
        GUIDANCE_SUBJECTS
    } else {
        GUIDANCE_NO_SUBJECTS
    };
    not_shown(resp, with_subjects)
        .into_iter()
        .chain([first, GUIDANCE_SEARCH, GUIDANCE_ABSENCE].map(String::from))
        .collect()
}

pub fn memories_heading(resp: &ContextResponse) -> String {
    let page = &resp.memories;
    let returned = page.items.len() as u64;
    if returned == 0 {
        format!("Memories (0 of {})", page.total)
    } else {
        format!(
            "Memories ({}-{} of {})",
            page.offset + 1,
            page.offset + returned,
            page.total
        )
    }
}

pub fn render_text(resp: &ContextResponse, with_subjects: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    let page = &resp.memories;
    if resp.instructions.is_empty() && page.total == 0 {
        lines.push(
            if with_subjects {
                "No instructions or memories for these subjects."
            } else {
                "No instructions or memories yet."
            }
            .into(),
        );
    } else {
        if !resp.instructions.is_empty() {
            lines.push(format!("Instructions ({})", resp.instructions.len()));
            lines.push(String::new());
            for i in &resp.instructions {
                let e = &i.entry;
                let bracket = if i.level == "team" {
                    "team-wide".to_string()
                } else {
                    subject_list(&i.matched)
                };
                lines.push(format!("[{bracket}] {}", e.index.title));
                lines.push(format!(
                    "instruction {}  rev {}  updated {} by {}",
                    e.index.id,
                    e.index.revision,
                    day(&e.index.updated_at),
                    author_display(&e.index.updated_by)
                ));
                lines.push(e.body.trim_end_matches('\n').to_string());
                lines.push(String::new());
            }
        }
        lines.push(memories_heading(resp));
        if page.items.is_empty() && page.offset >= page.total && page.total > 0 {
            lines.push(String::new());
            lines.push(format!(
                "No memories at this offset; there are {} (use --offset 0).",
                page.total
            ));
        } else if !page.items.is_empty() {
            lines.push(String::new());
        }
        for m in &page.items {
            let bracket = if m.level == "recent" {
                "recent".to_string()
            } else {
                subject_list(&m.matched)
            };
            lines.push(format!(
                "{}  {}  [{bracket}]  {}",
                m.index.id,
                day(&m.index.updated_at),
                m.index.title
            ));
            lines.push(format!("    {}", m.index.summary));
        }
    }
    lines.push(String::new());
    lines.extend(footer(resp, with_subjects));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "9a2f6c1e-7d3b-4c2a-9f1e-0b1c2d3e4f50";

    fn memory(level: &str) -> Value {
        json!({
            "id": ID, "kind": "memory", "title": "Checkout timeouts after deploys",
            "summary": "Retry storms follow each deploy.",
            "subjects": [{ "type": "service", "value": "checkout" }], "deleted": false,
            "revision": 1, "created_at": "2026-11-01T00:00:00Z", "updated_at": "2026-11-08T10:00:00Z",
            "updated_by": { "user_id": null, "name": null, "credential": "team_api_key" },
            "level": level,
            "matched": if level == "recent" { json!([]) } else { json!([{ "type": "service", "value": "checkout" }]) },
        })
    }

    fn resp(returned: usize, total: u64, offset: u64, team_wide: u64) -> ContextResponse {
        serde_json::from_value(json!({
            "instructions": [],
            "memories": {
                "items": (0..returned).map(|_| memory("subject")).collect::<Vec<_>>(),
                "total": total, "limit": 3, "offset": offset,
            },
            "team_wide_memories": team_wide,
            "memories_total": total + team_wide,
        }))
        .unwrap()
    }

    fn not_shown_line(r: &ContextResponse, subjects: bool) -> Option<String> {
        let f = footer(r, subjects);
        f.first().filter(|l| l.starts_with("Not shown")).cloned()
    }

    #[test]
    fn first_page() {
        let r = resp(3, 14, 0, 6);
        assert_eq!(memories_heading(&r), "Memories (1-3 of 14)");
        assert_eq!(
            not_shown_line(&r, true).unwrap(),
            "Not shown: 11 more memories for these subjects (--offset 3); 6 team-wide memories (cx context memories list --team-wide)."
        );
    }

    #[test]
    fn middle_page() {
        let r = resp(3, 14, 3, 6);
        assert_eq!(memories_heading(&r), "Memories (4-6 of 14)");
        assert_eq!(
            not_shown_line(&r, true).unwrap(),
            "Not shown: 8 more memories for these subjects (--offset 6); 6 team-wide memories (cx context memories list --team-wide)."
        );
    }

    #[test]
    fn last_page_without_team_wide() {
        let r = resp(2, 14, 12, 0);
        assert_eq!(memories_heading(&r), "Memories (13-14 of 14)");
        assert_eq!(not_shown_line(&r, true), None);
    }

    #[test]
    fn last_page_with_team_wide_only() {
        let r = resp(2, 14, 12, 4);
        assert_eq!(
            not_shown_line(&r, true).unwrap(),
            "Not shown: 4 team-wide memories (cx context memories list --team-wide)."
        );
    }

    #[test]
    fn offset_past_the_end() {
        let r = resp(0, 14, 20, 0);
        assert_eq!(memories_heading(&r), "Memories (0 of 14)");
        assert_eq!(not_shown_line(&r, true), None);
        assert!(render_text(&r, true)
            .contains("\nNo memories at this offset; there are 14 (use --offset 0).\n"));
    }

    #[test]
    fn zero_totals() {
        let r = resp(0, 0, 0, 0);
        assert_eq!(memories_heading(&r), "Memories (0 of 0)");
        assert_eq!(
            render_text(&r, true),
            [
                "No instructions or memories for these subjects.",
                "",
                GUIDANCE_SUBJECTS,
                GUIDANCE_SEARCH,
                GUIDANCE_ABSENCE,
            ]
            .join("\n")
        );
        assert!(render_text(&r, false).starts_with("No instructions or memories yet.\n"));
    }

    #[test]
    fn instructions_without_memories() {
        let r: ContextResponse = serde_json::from_value(json!({
            "instructions": [{ "level": "team", "matched": [], "entry": {
                "id": ID, "kind": "instruction", "title": "Paging", "summary": "s", "subjects": [],
                "deleted": false, "revision": 1, "created_at": "2026-11-01T00:00:00Z",
                "updated_at": "2026-11-02T00:00:00Z",
                "updated_by": { "user_id": null, "name": "Dana", "credential": "user_token" },
                "body": "Page the owner.\n", "source": null, "team_wide": true, "change_note": null,
            }}],
            "memories": { "items": [], "total": 0, "limit": 50, "offset": 0 },
            "team_wide_memories": 0, "memories_total": 0,
        }))
        .unwrap();
        assert_eq!(
            render_text(&r, true),
            [
                "Instructions (1)",
                "",
                "[team-wide] Paging",
                &format!("instruction {ID}  rev 1  updated 2026-11-02 by Dana"),
                "Page the owner.",
                "",
                "Memories (0 of 0)",
                "",
                GUIDANCE_SUBJECTS,
                GUIDANCE_SEARCH,
                GUIDANCE_ABSENCE,
            ]
            .join("\n")
        );
    }

    #[test]
    fn without_subjects() {
        let r = resp(3, 14, 0, 6);
        assert_eq!(
            footer(&r, false),
            [
                "Not shown: 11 more recent memories (--offset 3).",
                GUIDANCE_NO_SUBJECTS,
                GUIDANCE_SEARCH,
                GUIDANCE_ABSENCE,
            ]
        );
    }

    #[test]
    fn with_subjects_guidance() {
        let r = resp(1, 1, 0, 0);
        assert_eq!(
            footer(&r, true),
            [GUIDANCE_SUBJECTS, GUIDANCE_SEARCH, GUIDANCE_ABSENCE]
        );
    }

    #[test]
    fn hints_are_added_at_top_level() {
        let v = with_hints(json!({ "a": 1 }), vec!["x".into()]);
        assert_eq!(v, json!({ "a": 1, "hints": ["x"] }));
    }
}
