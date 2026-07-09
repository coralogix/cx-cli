//! REST client for the Coralogix AI Center (AI v3) configuration APIs.
//!
//! The proto (`com/coralogixapis/ai/v3`) annotates paths like `/ai/applications/v3`,
//! but openapi-facade serves them under the management gateway mount `/mgmt/openapi/5`
//! (the mount every management command uses) — hence the [`AI_BASE`] prefix. There is
//! deliberately no delete for custom-evaluation policies, applications, or pricing.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::api_client::CxClient;
use crate::error::Result;

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

/// Wrapper for the two custom-evaluation list routes, which both return `{ "items": [...] }`.
/// Items are kept as raw `Value` so JSON/agents output preserves every policy field
/// (config, instructions, examples, policyType, …); the text table reads what it needs.
#[derive(Debug, Default, Deserialize)]
pub struct CustomEvaluationItems {
    #[serde(default)]
    pub items: Vec<Value>,
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

    /// GET /ai/custom-evaluations/v3 — all custom evaluations (raw items).
    pub async fn list_custom_evaluations(&self) -> Result<Vec<Value>> {
        let resp: CustomEvaluationItems = self
            .client
            .get(&format!("{AI_BASE}/custom-evaluations/v3"), &[])
            .await?;
        Ok(resp.items)
    }

    /// GET /ai/custom-evaluations/v3/by-application/{application_id} (raw items).
    pub async fn list_custom_evaluations_for_application(
        &self,
        application_id: &str,
    ) -> Result<Vec<Value>> {
        let resp: CustomEvaluationItems = self
            .client
            .get(
                &format!("{AI_BASE}/custom-evaluations/v3/by-application/{application_id}"),
                &[],
            )
            .await?;
        Ok(resp.items)
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
        self.client
            .patch(&format!("{AI_BASE}/evaluations/v3/{id}"), body)
            .await
    }

    /// DELETE /ai/evaluations/v3/{id} — remove an evaluation from its application.
    pub async fn delete_evaluation(&self, id: &str) -> Result<Value> {
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
    fn custom_evaluation_items_preserve_full_object() {
        // Items are raw Values, so every policy field (incl. config/instructions)
        // survives for JSON/agents output, not just the columns the table reads.
        let body = json!({
            "items": [
                { "id": "c-1", "name": "No PII", "description": "block pii",
                  "applicationIds": ["a-1", "a-2"],
                  "config": { "instructions": "reject PII", "policyType": "SECURITY" } }
            ]
        });
        let resp: CustomEvaluationItems = serde_json::from_value(body).unwrap();
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.items[0]["name"], "No PII");
        assert_eq!(resp.items[0]["config"]["policyType"], "SECURITY");
    }
}
