use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceKind {
    Path,
    Command,
    Stat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: EvidenceKind,
    pub value: String,
    pub masked: bool,
}

impl Evidence {
    pub fn path(value: impl Into<String>, masked: bool) -> Self {
        Self {
            kind: EvidenceKind::Path,
            value: value.into(),
            masked,
        }
    }

    pub fn command(value: impl Into<String>) -> Self {
        Self {
            kind: EvidenceKind::Command,
            value: value.into(),
            masked: false,
        }
    }

    pub fn stat(value: impl Into<String>) -> Self {
        Self {
            kind: EvidenceKind::Stat,
            value: value.into(),
            masked: false,
        }
    }
}
