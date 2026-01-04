use crate::core::RiskLevel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRef {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ActionKind {
    #[serde(rename = "TRASH_MOVE")]
    TrashMove { paths: Vec<String> },
    #[serde(rename = "DELETE")]
    Delete { paths: Vec<String> },
    #[serde(rename = "RUN_CMD")]
    RunCmd { cmd: String, args: Vec<String> },
    #[serde(rename = "OPEN_IN_FINDER")]
    OpenInFinder { path: String },
    #[serde(rename = "SHOW_INSTRUCTIONS")]
    ShowInstructions { markdown: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPlan {
    pub id: String,
    pub title: String,
    pub risk_level: RiskLevel,
    pub estimated_reclaimed_bytes: u64,
    pub related_findings: Vec<String>,
    pub kind: ActionKind,
    pub notes: Vec<String>,
}
