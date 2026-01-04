use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskLevel {
    R0,
    R1,
    R2,
    R3,
}

impl RiskLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            RiskLevel::R0 => "R0",
            RiskLevel::R1 => "R1",
            RiskLevel::R2 => "R2",
            RiskLevel::R3 => "R3",
        }
    }
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RiskLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let s = s.strip_prefix("<=").unwrap_or(s).trim();
        match s.to_ascii_uppercase().as_str() {
            "R0" => Ok(RiskLevel::R0),
            "R1" => Ok(RiskLevel::R1),
            "R2" => Ok(RiskLevel::R2),
            "R3" => Ok(RiskLevel::R3),
            _ => Err(format!(
                "リスクレベルが不正です: {s}（R0|R1|R2|R3 を指定してください）"
            )),
        }
    }
}
