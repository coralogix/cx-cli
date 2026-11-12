use anyhow::{anyhow, Result};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api_client::{extract_error_detail, CxClient};
use crate::error::CxError;

const BASE: &str = "/mgmt/api/v1/agent-context";

const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_');

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Kind {
    Memory,
    Instruction,
}

impl Kind {
    /// `memory` / `instruction`: the JSON value and the word in messages.
    pub fn singular(self) -> &'static str {
        match self {
            Kind::Memory => "memory",
            Kind::Instruction => "instruction",
        }
    }

    /// `memories` / `instructions`: the path segment and the `cx context` sub-noun.
    pub fn noun(self) -> &'static str {
        match self {
            Kind::Memory => "memories",
            Kind::Instruction => "instructions",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subject {
    #[serde(rename = "type")]
    pub subject_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Author {
    pub user_id: Option<String>,
    pub name: Option<String>,
    pub credential: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IndexLine {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub subjects: Vec<Subject>,
    pub deleted: bool,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
    pub updated_by: Author,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    #[serde(flatten)]
    pub index: IndexLine,
    pub body: String,
    pub source: Option<String>,
    pub team_wide: bool,
    pub change_note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FoundBy {
    pub context_for: Vec<Subject>,
    pub team_wide: bool,
    pub returned_in_full: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Receipt {
    pub entry: Entry,
    pub found_by: FoundBy,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RevisionItem {
    pub revision: u64,
    pub operation: String,
    pub reverted_to: Option<u64>,
    pub change_note: Option<String>,
    pub author: Author,
    pub client: Option<String>,
    pub created_at: String,
    pub title: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TeamRevisionItem {
    #[serde(flatten)]
    pub revision: RevisionItem,
    pub id: String,
    pub kind: String,
    pub current_revision: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SubjectTypeCount {
    #[serde(rename = "type")]
    pub subject_type: String,
    pub memories: u64,
    pub instructions: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubjectTypes {
    pub types: Vec<SubjectTypeCount>,
    pub total: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub limit: u64,
    pub offset: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContextInstruction {
    pub level: String,
    pub matched: Vec<Subject>,
    pub entry: Entry,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContextMemory {
    #[serde(flatten)]
    pub index: IndexLine,
    pub level: String,
    pub matched: Vec<Subject>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContextResponse {
    pub instructions: Vec<ContextInstruction>,
    pub memories: Page<ContextMemory>,
    pub team_wide_memories: u64,
    pub memories_total: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchMatch {
    pub field: String,
    pub line: u64,
    pub text: String,
    pub cut_start: bool,
    pub cut_end: bool,
}

/// A list or search item; `match_count` and `matches` come only with patterns.
#[derive(Debug, Clone, Deserialize)]
pub struct QueryItem {
    #[serde(flatten)]
    pub index: IndexLine,
    #[serde(default)]
    pub match_count: Option<u64>,
    #[serde(default)]
    pub matches: Vec<SearchMatch>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextRequest {
    pub subjects: Vec<Subject>,
    pub limit: u32,
    pub offset: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct QueryRequest {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fixed: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub subjects: Vec<Subject>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub team_wide: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub include_deleted: bool,
    pub limit: u32,
    pub offset: u64,
}

/// The create body; `update` sends the same fields without `id`, plus its own.
#[derive(Debug, Clone, Serialize)]
pub struct EntryContent {
    pub title: String,
    pub summary: String,
    pub body: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub subjects: Vec<Subject>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub team_wide: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateRequest {
    pub id: String,
    #[serde(flatten)]
    pub content: EntryContent,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateRequest {
    #[serde(flatten)]
    pub content: EntryContent,
    pub expected_revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_note: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct HistoryQuery {
    pub author: Option<String>,
    pub source: Option<String>,
    pub kind: Option<Kind>,
    /// RFC 3339.
    pub since: Option<String>,
    pub limit: u32,
    pub offset: u64,
}

/// A successful response: the status, the body as the service sent it (printed
/// unchanged by `-o json`), and the typed view of it.
#[derive(Debug, Clone)]
pub struct Reply<T> {
    pub status: u16,
    pub raw: Value,
    pub data: T,
}

/// What a failure is about, for the messages that name the entry.
#[derive(Debug, Clone, Copy, Default)]
pub struct ErrorScope<'a> {
    pub kind: Option<Kind>,
    pub id: Option<&'a str>,
    pub expected_revision: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct ServiceError {
    code: String,
    message: String,
    #[serde(default)]
    field: Option<String>,
    #[serde(default)]
    current_revision: Option<u64>,
}

pub const NOT_AVAILABLE: &str = "Agent context is not available in this environment.";

pub fn revision_conflict_message(kind: Kind, id: &str, current: u64, expected: u64) -> String {
    format!(
        "The {k} changed: it is at revision {current}, not {expected}. Read it again (cx context {n} get {id}) and retry with --expected-revision {current}.",
        k = kind.singular(),
        n = kind.noun(),
    )
}

pub fn deleted_message(kind: Kind, id: &str) -> String {
    format!(
        "The {k} is deleted. See cx context {n} history {id}, then restore it with revert.",
        k = kind.singular(),
        n = kind.noun(),
    )
}

/// Turns a non-2xx status and body into the contract's message.
pub fn decode_failure(status: u16, body: &str, scope: ErrorScope<'_>) -> anyhow::Error {
    let Ok(err) = serde_json::from_str::<ServiceError>(body) else {
        if status == 404 {
            return anyhow!(NOT_AVAILABLE);
        }
        return CxError::Api {
            status,
            message: extract_error_detail(body).unwrap_or_else(|| body.to_string()),
        }
        .into();
    };
    let service_message = || match &err.field {
        Some(field) => format!("{field}: {}", err.message),
        None => err.message.clone(),
    };
    let message = match (err.code.as_str(), scope.kind, scope.id) {
        ("revision_conflict", Some(kind), Some(id)) => {
            match (err.current_revision, scope.expected_revision) {
                (Some(current), Some(expected)) => {
                    revision_conflict_message(kind, id, current, expected)
                }
                _ => service_message(),
            }
        }
        ("id_conflict", Some(kind), _) => format!(
            "A {} or another entry with this ID exists with different content. Omit --id to create a new entry.",
            kind.singular()
        ),
        ("deleted", Some(kind), Some(id)) => deleted_message(kind, id),
        ("not_found", Some(kind), Some(id)) => format!(
            "No such {} {id} in this team (or no such revision).",
            kind.singular()
        ),
        _ => service_message(),
    };
    anyhow!(message)
}

fn encode_segment(id: &str) -> String {
    utf8_percent_encode(id, PATH_SEGMENT_ENCODE_SET).to_string()
}

fn entry_path(kind: Kind, id: &str) -> String {
    format!("{BASE}/{}/{}", kind.noun(), encode_segment(id))
}

async fn call<T: DeserializeOwned>(
    client: &CxClient,
    method: Method,
    path: &str,
    query: &[(&str, &str)],
    body: Option<Value>,
    scope: ErrorScope<'_>,
) -> Result<Reply<T>> {
    let (status, text) = client
        .send_with_status(method, path, query, body.as_ref())
        .await?;
    if !(200..300).contains(&status) {
        return Err(decode_failure(status, &text, scope));
    }
    let raw: Value = serde_json::from_str(&text)?;
    let data = serde_json::from_value(raw.clone())?;
    Ok(Reply { status, raw, data })
}

fn kind_scope(kind: Kind) -> ErrorScope<'static> {
    ErrorScope {
        kind: Some(kind),
        ..ErrorScope::default()
    }
}

fn entry_scope(kind: Kind, id: &str, expected_revision: Option<u64>) -> ErrorScope<'_> {
    ErrorScope {
        kind: Some(kind),
        id: Some(id),
        expected_revision,
    }
}

pub async fn read_context(
    client: &CxClient,
    req: &ContextRequest,
) -> Result<Reply<ContextResponse>> {
    let path = format!("{BASE}/context");
    let body = serde_json::to_value(req)?;
    call(
        client,
        Method::POST,
        &path,
        &[],
        Some(body),
        ErrorScope::default(),
    )
    .await
}

pub async fn team_history(
    client: &CxClient,
    q: &HistoryQuery,
) -> Result<Reply<Page<TeamRevisionItem>>> {
    let path = format!("{BASE}/history");
    let limit = q.limit.to_string();
    let offset = q.offset.to_string();
    let mut query: Vec<(&str, &str)> = vec![("limit", &limit), ("offset", &offset)];
    if let Some(author) = &q.author {
        query.push(("author", author));
    }
    if let Some(source) = &q.source {
        query.push(("source", source));
    }
    if let Some(kind) = q.kind {
        query.push(("kind", kind.singular()));
    }
    if let Some(since) = &q.since {
        query.push(("since", since));
    }
    call(
        client,
        Method::GET,
        &path,
        &query,
        None,
        ErrorScope::default(),
    )
    .await
}

pub async fn subject_types(client: &CxClient) -> Result<Reply<SubjectTypes>> {
    let path = format!("{BASE}/subject-types");
    call(client, Method::GET, &path, &[], None, ErrorScope::default()).await
}

pub async fn query_entries(
    client: &CxClient,
    kind: Kind,
    req: &QueryRequest,
) -> Result<Reply<Page<QueryItem>>> {
    let path = format!("{BASE}/{}/query", kind.noun());
    let body = serde_json::to_value(req)?;
    call(
        client,
        Method::POST,
        &path,
        &[],
        Some(body),
        kind_scope(kind),
    )
    .await
}

pub async fn get_entry(
    client: &CxClient,
    kind: Kind,
    id: &str,
    revision: Option<u64>,
) -> Result<Reply<Entry>> {
    let revision = revision.map(|r| r.to_string());
    let query: Vec<(&str, &str)> = revision.iter().map(|r| ("revision", r.as_str())).collect();
    let path = entry_path(kind, id);
    call(
        client,
        Method::GET,
        &path,
        &query,
        None,
        entry_scope(kind, id, None),
    )
    .await
}

pub async fn create_entry(
    client: &CxClient,
    kind: Kind,
    req: &CreateRequest,
) -> Result<Reply<Receipt>> {
    let path = format!("{BASE}/{}", kind.noun());
    let body = serde_json::to_value(req)?;
    let scope = entry_scope(kind, &req.id, None);
    call(client, Method::POST, &path, &[], Some(body), scope).await
}

pub async fn update_entry(
    client: &CxClient,
    kind: Kind,
    id: &str,
    req: &UpdateRequest,
) -> Result<Reply<Receipt>> {
    let path = entry_path(kind, id);
    let body = serde_json::to_value(req)?;
    let scope = entry_scope(kind, id, Some(req.expected_revision));
    call(client, Method::PUT, &path, &[], Some(body), scope).await
}

pub async fn delete_entry(
    client: &CxClient,
    kind: Kind,
    id: &str,
    expected_revision: u64,
) -> Result<Reply<Entry>> {
    let path = entry_path(kind, id);
    let expected = expected_revision.to_string();
    let query = [("expected_revision", expected.as_str())];
    let scope = entry_scope(kind, id, Some(expected_revision));
    call(client, Method::DELETE, &path, &query, None, scope).await
}

pub async fn entry_revisions(
    client: &CxClient,
    kind: Kind,
    id: &str,
    limit: u32,
    offset: u64,
) -> Result<Reply<Page<RevisionItem>>> {
    let path = format!("{}/revisions", entry_path(kind, id));
    let limit = limit.to_string();
    let offset = offset.to_string();
    let query = [("limit", limit.as_str()), ("offset", offset.as_str())];
    call(
        client,
        Method::GET,
        &path,
        &query,
        None,
        entry_scope(kind, id, None),
    )
    .await
}

pub async fn revert_entry(
    client: &CxClient,
    kind: Kind,
    id: &str,
    to_revision: u64,
    expected_revision: u64,
) -> Result<Reply<Receipt>> {
    let path = format!("{}/revert", entry_path(kind, id));
    let body = serde_json::json!({
        "to_revision": to_revision,
        "expected_revision": expected_revision,
    });
    let scope = entry_scope(kind, id, Some(expected_revision));
    call(client, Method::POST, &path, &[], Some(body), scope).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn author_json(name: Option<&str>) -> Value {
        json!({ "user_id": name.map(|_| "u-1"), "name": name, "credential": if name.is_some() { "user_token" } else { "team_api_key" } })
    }

    fn index_json(id: &str, kind: &str, subjects: Value) -> Value {
        json!({
            "id": id, "kind": kind, "title": "Checkout timeouts after deploys",
            "summary": "Retry storms follow each deploy; check the pool size first.",
            "subjects": subjects, "deleted": false, "revision": 3,
            "created_at": "2026-11-01T09:00:00.000Z", "updated_at": "2026-11-08T10:11:12.345Z",
            "updated_by": author_json(Some("Dana")),
        })
    }

    fn entry_json(id: &str, kind: &str, subjects: Value, source: Option<&str>) -> Value {
        let mut v = index_json(id, kind, subjects.clone());
        let team_wide = subjects.as_array().is_some_and(|s| s.is_empty());
        v["body"] = json!("line one\nline two");
        v["source"] = json!(source);
        v["team_wide"] = json!(team_wide);
        v["change_note"] = Value::Null;
        v
    }

    const ID: &str = "9a2f6c1e-7d3b-4c2a-9f1e-0b1c2d3e4f50";

    #[test]
    fn deserializes_entry_with_nulls_and_type_only_subject() {
        let raw = entry_json(
            ID,
            "memory",
            json!([{ "type": "service", "value": "checkout" }, { "type": "alert", "value": null }]),
            None,
        );
        let e: Entry = serde_json::from_value(raw).unwrap();
        assert_eq!(e.index.id, ID);
        assert_eq!(e.index.revision, 3);
        assert_eq!(e.index.updated_by.name.as_deref(), Some("Dana"));
        assert_eq!(e.index.subjects[1].value, None);
        assert_eq!(e.source, None);
        assert_eq!(e.change_note, None);
        assert!(!e.team_wide);
        assert_eq!(e.body, "line one\nline two");
    }

    #[test]
    fn deserializes_receipt() {
        let raw = json!({
            "entry": entry_json(ID, "instruction", json!([]), Some("incident review")),
            "found_by": { "context_for": [], "team_wide": true, "returned_in_full": true },
        });
        let r: Receipt = serde_json::from_value(raw).unwrap();
        assert!(r.entry.team_wide);
        assert_eq!(r.entry.source.as_deref(), Some("incident review"));
        assert!(r.found_by.team_wide && r.found_by.returned_in_full);
        assert!(r.found_by.context_for.is_empty());
    }

    #[test]
    fn deserializes_context_response() {
        let raw = json!({
            "instructions": [
                { "level": "team", "matched": [], "entry": entry_json(ID, "instruction", json!([]), None) },
                { "level": "subject", "matched": [{ "type": "alert", "value": "8f3a" }],
                  "entry": entry_json(ID, "instruction", json!([{ "type": "alert", "value": "8f3a" }]), None) },
            ],
            "memories": {
                "items": [{
                    "id": ID, "kind": "memory", "title": "t", "summary": "s",
                    "subjects": [{ "type": "service", "value": "checkout" }], "deleted": false,
                    "revision": 1, "created_at": "2026-11-01T00:00:00.000Z",
                    "updated_at": "2026-11-01T00:00:00.000Z",
                    "updated_by": author_json(None),
                    "level": "subject", "matched": [{ "type": "service", "value": "checkout" }],
                }],
                "total": 14, "limit": 3, "offset": 3,
            },
            "team_wide_memories": 6,
            "memories_total": 40,
        });
        let c: ContextResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(c.instructions.len(), 2);
        assert_eq!(c.instructions[0].level, "team");
        assert_eq!(c.memories.items[0].level, "subject");
        assert_eq!(c.memories.items[0].index.updated_by.name, None);
        assert_eq!(
            c.memories.items[0].index.updated_by.credential,
            "team_api_key"
        );
        assert_eq!(
            (c.memories.total, c.memories.limit, c.memories.offset),
            (14, 3, 3)
        );
        assert_eq!((c.team_wide_memories, c.memories_total), (6, 40));
    }

    #[test]
    fn deserializes_list_and_search_pages() {
        let list = json!({ "items": [index_json(ID, "memory", json!([]))], "total": 1, "limit": 50, "offset": 0 });
        let p: Page<QueryItem> = serde_json::from_value(list).unwrap();
        assert_eq!(p.items[0].match_count, None);
        assert!(p.items[0].matches.is_empty());

        let mut item = index_json(ID, "memory", json!([]));
        item["match_count"] = json!(5);
        item["matches"] = json!([
            { "field": "title", "line": 1, "text": "Checkout timeouts", "cut_start": false, "cut_end": false },
            { "field": "body", "line": 7, "text": "…timed out", "cut_start": true, "cut_end": true },
        ]);
        let search = json!({ "items": [item], "total": 1, "limit": 50, "offset": 0 });
        let p: Page<QueryItem> = serde_json::from_value(search).unwrap();
        assert_eq!(p.items[0].match_count, Some(5));
        assert_eq!(p.items[0].matches[1].field, "body");
        assert_eq!(p.items[0].matches[1].line, 7);
        assert!(p.items[0].matches[1].cut_start && p.items[0].matches[1].cut_end);
    }

    #[test]
    fn deserializes_revision_pages() {
        let rev = json!({
            "revision": 4, "operation": "revert", "reverted_to": 2,
            "change_note": "reverted to revision 2", "author": author_json(None),
            "client": null, "created_at": "2026-11-08T10:11:12.345Z", "title": "t", "deleted": true,
        });
        let p: Page<RevisionItem> = serde_json::from_value(
            json!({ "items": [rev.clone()], "total": 4, "limit": 50, "offset": 0 }),
        )
        .unwrap();
        assert_eq!(p.items[0].reverted_to, Some(2));
        assert_eq!(p.items[0].client, None);
        assert!(p.items[0].deleted);

        let mut team = rev;
        team["id"] = json!(ID);
        team["kind"] = json!("instruction");
        team["current_revision"] = json!(5);
        team["reverted_to"] = Value::Null;
        let p: Page<TeamRevisionItem> =
            serde_json::from_value(json!({ "items": [team], "total": 9, "limit": 1, "offset": 8 }))
                .unwrap();
        assert_eq!(p.items[0].id, ID);
        assert_eq!(p.items[0].current_revision, 5);
        assert_eq!(p.items[0].revision.reverted_to, None);
    }

    #[test]
    fn serializes_requests_in_service_shape() {
        let content = EntryContent {
            title: "t".into(),
            summary: "s".into(),
            body: "b".into(),
            subjects: vec![Subject {
                subject_type: "alert".into(),
                value: None,
            }],
            team_wide: false,
            source: None,
        };
        let create = serde_json::to_value(CreateRequest {
            id: ID.into(),
            content: content.clone(),
        })
        .unwrap();
        assert_eq!(
            create,
            json!({ "id": ID, "title": "t", "summary": "s", "body": "b", "subjects": [{ "type": "alert" }] })
        );
        let update = serde_json::to_value(UpdateRequest {
            content: EntryContent {
                subjects: vec![],
                team_wide: true,
                source: Some("x".into()),
                ..content
            },
            expected_revision: 2,
            change_note: None,
        })
        .unwrap();
        assert_eq!(
            update,
            json!({ "title": "t", "summary": "s", "body": "b", "team_wide": true, "source": "x", "expected_revision": 2 })
        );
        let query = serde_json::to_value(QueryRequest {
            limit: 50,
            ..QueryRequest::default()
        })
        .unwrap();
        assert_eq!(query, json!({ "limit": 50, "offset": 0 }));
    }

    fn msg(status: u16, body: &str, scope: ErrorScope<'_>) -> String {
        decode_failure(status, body, scope).to_string()
    }

    fn with_id(kind: Kind, expected: Option<u64>) -> ErrorScope<'static> {
        ErrorScope {
            kind: Some(kind),
            id: Some(ID),
            expected_revision: expected,
        }
    }

    #[test]
    fn decodes_revision_conflict_for_both_kinds() {
        let body = r#"{"code":"revision_conflict","message":"expected revision 2, current 5","current_revision":5}"#;
        assert_eq!(
            msg(409, body, with_id(Kind::Memory, Some(2))),
            format!("The memory changed: it is at revision 5, not 2. Read it again (cx context memories get {ID}) and retry with --expected-revision 5.")
        );
        assert_eq!(
            msg(409, body, with_id(Kind::Instruction, Some(2))),
            format!("The instruction changed: it is at revision 5, not 2. Read it again (cx context instructions get {ID}) and retry with --expected-revision 5.")
        );
    }

    #[test]
    fn decodes_id_conflict_deleted_and_not_found_for_both_kinds() {
        for kind in [Kind::Memory, Kind::Instruction] {
            let (k, n) = (kind.singular(), kind.noun());
            assert_eq!(
                msg(409, r#"{"code":"id_conflict","message":"x"}"#, with_id(kind, None)),
                format!("A {k} or another entry with this ID exists with different content. Omit --id to create a new entry.")
            );
            assert_eq!(
                msg(409, r#"{"code":"deleted","message":"x"}"#, with_id(kind, Some(3))),
                format!("The {k} is deleted. See cx context {n} history {ID}, then restore it with revert.")
            );
            assert_eq!(
                msg(
                    404,
                    r#"{"code":"not_found","message":"entry not found"}"#,
                    with_id(kind, None)
                ),
                format!("No such {k} {ID} in this team (or no such revision).")
            );
        }
    }

    #[test]
    fn structured_not_found_without_id_keeps_service_message() {
        let body = r#"{"code":"not_found","message":"no route for GET /x"}"#;
        assert_eq!(
            msg(404, body, kind_scope(Kind::Memory)),
            "no route for GET /x"
        );
        assert_eq!(msg(404, body, ErrorScope::default()), "no route for GET /x");
    }

    #[test]
    fn other_codes_show_service_message_with_field() {
        let scope = kind_scope(Kind::Memory);
        assert_eq!(
            msg(
                400,
                r#"{"code":"invalid_pattern","message":"invalid regular expression: brackets [] not balanced"}"#,
                scope
            ),
            "invalid regular expression: brackets [] not balanced"
        );
        assert_eq!(
            msg(
                400,
                r#"{"code":"pattern_timeout","message":"search took too long"}"#,
                scope
            ),
            "search took too long"
        );
        assert_eq!(
            msg(
                400,
                r#"{"code":"invalid_argument","message":"must be 1..200 characters","field":"title"}"#,
                scope
            ),
            "title: must be 1..200 characters"
        );
        assert_eq!(
            msg(
                500,
                r#"{"code":"internal","message":"internal error","field":null}"#,
                scope
            ),
            "internal error"
        );
    }

    #[test]
    fn unstructured_404_is_not_available() {
        assert_eq!(
            msg(404, "404 page not found", ErrorScope::default()),
            NOT_AVAILABLE
        );
        assert_eq!(
            msg(
                404,
                r#"{"message":"Not Found"}"#,
                with_id(Kind::Memory, None)
            ),
            NOT_AVAILABLE
        );
        assert_eq!(msg(404, "", ErrorScope::default()), NOT_AVAILABLE);
    }

    #[test]
    fn unstructured_failure_is_generic() {
        assert_eq!(
            msg(502, "<html>Bad Gateway</html>", ErrorScope::default()),
            "API request failed (502): <html>Bad Gateway</html>"
        );
        assert_eq!(
            msg(
                503,
                r#"{"message":"upstream unavailable"}"#,
                with_id(Kind::Memory, None)
            ),
            "API request failed (503): upstream unavailable"
        );
    }

    #[test]
    fn revision_conflict_without_numbers_falls_back_to_service_message() {
        let body = r#"{"code":"revision_conflict","message":"stale"}"#;
        assert_eq!(msg(409, body, with_id(Kind::Memory, Some(2))), "stale");
    }

    #[test]
    fn entry_path_encodes_id() {
        assert_eq!(
            entry_path(Kind::Memory, ID),
            format!("{BASE}/memories/{ID}")
        );
        assert_eq!(
            entry_path(Kind::Instruction, "a/b c"),
            format!("{BASE}/instructions/a%2Fb%20c")
        );
    }
}
