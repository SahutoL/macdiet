use crate::core::{ActionPlan, Finding};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportSummary {
    pub estimated_total_bytes: u64,
    pub unobserved_bytes: u64,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: String,
    pub tool_version: String,
    pub os: OsInfo,
    pub generated_at: String,
    pub summary: ReportSummary,
    pub findings: Vec<Finding>,
    pub actions: Vec<ActionPlan>,
}
