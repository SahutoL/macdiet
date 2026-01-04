use crate::core::{ActionRef, Evidence, RiskLevel};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    #[serde(rename = "type")]
    pub finding_type: String,
    pub title: String,
    pub estimated_bytes: u64,
    pub confidence: f64,
    pub risk_level: RiskLevel,
    pub evidence: Vec<Evidence>,
    pub recommended_actions: Vec<ActionRef>,
}
