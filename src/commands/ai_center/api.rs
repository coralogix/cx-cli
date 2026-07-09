//! REST client for the Coralogix AI Center (AI v3) configuration APIs.
//!
//! The proto (`com/coralogixapis/ai/v3`) annotates paths like `/ai/applications/v3`,
//! but openapi-facade serves them under the management gateway mount `/mgmt/openapi/5`
//! (the mount every management command uses) — hence the [`AI_BASE`] prefix. There is
//! deliberately no delete for custom-evaluation policies, applications, or pricing.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::api_client::CxClient;
use crate::error::{CxError, Result};

/// openapi-facade management gateway mount that fronts the AI v3 proto routes.
const AI_BASE: &str = "/mgmt/openapi/5/ai";

// --- Response types (only the fields the text tables render; JSON/agents pass raw) ---

/// One AI application (a GenAI `application` + `subsystem` pair).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiApplication {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub application: Option<String>,
    #[serde(default)]
    pub subsystem: Option<String>,
    #[serde(default)]
    pub guardrails_integrated: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListApplicationsResponse {
    #[serde(default)]
    pub ai_applications: Vec<AiApplication>,
}

/// One configured evaluation/policy on an application.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiEvaluation {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub application: Option<String>,
    #[serde(default)]
    pub subsystem: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub is_enabled: Option<bool>,
    #[serde(default)]
    pub threshold: Option<f64>,
    /// Type-specific config (a proto `oneof`); kept raw. The variant key names the type.
    #[serde(default)]
    pub config: Value,
}

impl AiEvaluation {
    /// Best-effort evaluation-type label from the `config` oneof's variant key.
    pub fn config_type(&self) -> String {
        self.config
            .as_object()
            .and_then(|m| m.keys().next())
            .cloned()
            .unwrap_or_else(|| "-".to_string())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEvaluationsResponse {
    #[serde(default)]
    pub ai_evaluations: Vec<AiEvaluation>,
}

/// One custom evaluation (policy).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEvaluation {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub application_ids: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListCustomEvaluationsResponse {
    /// Both `GET /custom-evaluations/v3` and the `by-application` route wrap in `items`.
    #[serde(default)]
    pub items: Vec<CustomEvaluation>,
}

/// Reject a blank path-segment id before building a URL, so an empty/whitespace id
/// can't collapse to an empty path segment and silently hit the wrong route.
fn require_id(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(CxError::Invalid(format!("{name} is required")));
    }
    Ok(())
}

// --- API ---

pub struct AiCenterApi<'a> {
    client: &'a CxClient,
}

impl<'a> AiCenterApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    // ── Reads ────────────────────────────────────────────────────────

    /// GET /ai/applications/v3 — list AI applications (incl. `guardrailsIntegrated`).
    pub async fn list_applications(
        &self,
        params: &[(&str, &str)],
    ) -> Result<ListApplicationsResponse> {
        self.client
            .get(&format!("{AI_BASE}/applications/v3"), params)
            .await
    }

    /// GET /ai/applications/v3/{id}.
    pub async fn get_application(&self, id: &str) -> Result<Value> {
        require_id(id, "application_id")?;
        self.client
            .get(&format!("{AI_BASE}/applications/v3/{id}"), &[])
            .await
    }

    /// GET /ai/evaluations/v3 — configured evaluations/policies (optionally per app).
    pub async fn list_evaluations(
        &self,
        params: &[(&str, &str)],
    ) -> Result<ListEvaluationsResponse> {
        self.client
            .get(&format!("{AI_BASE}/evaluations/v3"), params)
            .await
    }

    /// GET /ai/evaluations/v3/{id}.
    pub async fn get_evaluation(&self, id: &str) -> Result<Value> {
        require_id(id, "evaluation_id")?;
        self.client
            .get(&format!("{AI_BASE}/evaluations/v3/{id}"), &[])
            .await
    }

    /// GET /ai/evaluation-counts/v3/per-type — app count per evaluation type (coverage).
    pub async fn count_apps_per_eval_type(&self) -> Result<Value> {
        self.client
            .get(&format!("{AI_BASE}/evaluation-counts/v3/per-type"), &[])
            .await
    }

    /// GET /ai/custom-evaluations/v3 — all custom evaluations.
    pub async fn list_custom_evaluations(&self) -> Result<ListCustomEvaluationsResponse> {
        self.client
            .get(&format!("{AI_BASE}/custom-evaluations/v3"), &[])
            .await
    }

    /// GET /ai/custom-evaluations/v3/by-application/{application_id}.
    pub async fn list_custom_evaluations_for_application(
        &self,
        application_id: &str,
    ) -> Result<ListCustomEvaluationsResponse> {
        require_id(application_id, "application_id")?;
        self.client
            .get(
                &format!("{AI_BASE}/custom-evaluations/v3/by-application/{application_id}"),
                &[],
            )
            .await
    }

    /// GET /ai/model-pricing/v3 — the team's custom model pricing overrides.
    pub async fn get_model_pricing(&self) -> Result<Value> {
        self.client
            .get(&format!("{AI_BASE}/model-pricing/v3"), &[])
            .await
    }

    // ── Writes ───────────────────────────────────────────────────────

    /// POST /ai/evaluations/v3 — create (enable) an evaluation on an application.
    pub async fn create_evaluation(&self, body: &Value) -> Result<Value> {
        self.client
            .post(&format!("{AI_BASE}/evaluations/v3"), body)
            .await
    }

    /// PATCH /ai/evaluations/v3/{id}.
    pub async fn update_evaluation(&self, id: &str, body: &Value) -> Result<Value> {
        require_id(id, "evaluation_id")?;
        self.client
            .patch(&format!("{AI_BASE}/evaluations/v3/{id}"), body)
            .await
    }

    /// DELETE /ai/evaluations/v3/{id} — remove an evaluation from its application.
    pub async fn delete_evaluation(&self, id: &str) -> Result<Value> {
        require_id(id, "evaluation_id")?;
        self.client
            .delete(&format!("{AI_BASE}/evaluations/v3/{id}"))
            .await
    }

    /// POST /ai/custom-evaluations/v3 — create a custom evaluation (policy).
    pub async fn create_custom_evaluation(&self, body: &Value) -> Result<Value> {
        self.client
            .post(&format!("{AI_BASE}/custom-evaluations/v3"), body)
            .await
    }

    /// PATCH /ai/custom-evaluations/v3/{id}.
    pub async fn update_custom_evaluation(&self, id: &str, body: &Value) -> Result<Value> {
        require_id(id, "evaluation_id")?;
        self.client
            .patch(&format!("{AI_BASE}/custom-evaluations/v3/{id}"), body)
            .await
    }

    /// POST /ai/custom-evaluations/v3/{id}/applications/{application_id} — attach a
    /// custom evaluation (policy) to an application (the LinkCustomEvaluation RPC).
    pub async fn add_policy_to_application(
        &self,
        evaluation_id: &str,
        application_id: &str,
    ) -> Result<Value> {
        require_id(evaluation_id, "evaluation_id")?;
        require_id(application_id, "application_id")?;
        self.client
            .post_empty(
                &format!(
                    "{AI_BASE}/custom-evaluations/v3/{evaluation_id}/applications/{application_id}"
                ),
                &[],
            )
            .await
    }

    /// DELETE /ai/custom-evaluations/v3/{id}/applications/{application_id} — detach a
    /// custom evaluation (policy) from an application (reversible; the policy survives).
    pub async fn remove_policy_from_application(
        &self,
        evaluation_id: &str,
        application_id: &str,
    ) -> Result<Value> {
        require_id(evaluation_id, "evaluation_id")?;
        require_id(application_id, "application_id")?;
        self.client
            .delete(&format!(
                "{AI_BASE}/custom-evaluations/v3/{evaluation_id}/applications/{application_id}"
            ))
            .await
    }

    /// PUT /ai/model-pricing/v3 — set the team's per-model pricing overrides.
    /// `prices` is the model→price map; it is wrapped as `{"prices": …}` for the API.
    pub async fn set_model_pricing(&self, prices: &Value) -> Result<Value> {
        let body = json!({ "prices": prices });
        self.client
            .put(&format!("{AI_BASE}/model-pricing/v3"), &body)
            .await
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_list_applications() {
        let body = json!({
            "aiApplications": [
                { "id": "a-1", "application": "Production", "subsystem": "Advisor", "guardrailsIntegrated": true },
                { "id": "a-2", "application": "Staging", "subsystem": "Chatbot", "guardrailsIntegrated": false }
            ]
        });
        let resp: ListApplicationsResponse = serde_json::from_value(body).unwrap();
        assert_eq!(resp.ai_applications.len(), 2);
        assert_eq!(resp.ai_applications[0].id.as_deref(), Some("a-1"));
        assert_eq!(resp.ai_applications[0].guardrails_integrated, Some(true));
        assert_eq!(resp.ai_applications[1].guardrails_integrated, Some(false));
    }

    #[test]
    fn deserialize_empty_applications() {
        let resp: ListApplicationsResponse = serde_json::from_value(json!({})).unwrap();
        assert!(resp.ai_applications.is_empty());
    }

    #[test]
    fn deserialize_list_evaluations_and_config_type() {
        let body = json!({
            "aiEvaluations": [
                { "id": "e-1", "application": "Prod", "subsystem": "Sub", "isEnabled": true,
                  "target": "RESPONSE", "threshold": 0.8, "config": { "toxicity": {} } },
                { "id": "e-2", "application": "Prod", "subsystem": "Sub" }
            ]
        });
        let resp: ListEvaluationsResponse = serde_json::from_value(body).unwrap();
        assert_eq!(resp.ai_evaluations.len(), 2);
        assert_eq!(resp.ai_evaluations[0].config_type(), "toxicity");
        assert_eq!(resp.ai_evaluations[0].is_enabled, Some(true));
        assert_eq!(resp.ai_evaluations[1].config_type(), "-");
        assert_eq!(resp.ai_evaluations[1].is_enabled, None);
    }

    #[test]
    fn deserialize_custom_evaluations() {
        let body = json!({
            "items": [
                { "id": "c-1", "name": "No PII", "description": "block pii", "applicationIds": ["a-1", "a-2"] }
            ]
        });
        let resp: ListCustomEvaluationsResponse = serde_json::from_value(body).unwrap();
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.items[0].name.as_deref(), Some("No PII"));
        assert_eq!(resp.items[0].application_ids.len(), 2);
    }

    #[test]
    fn require_id_rejects_blank() {
        assert!(require_id("", "application_id").is_err());
        assert!(require_id("   ", "evaluation_id").is_err());
        assert!(require_id("abc", "evaluation_id").is_ok());
    }
}
