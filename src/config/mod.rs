use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::core::RiskLevel;

#[derive(Debug, Clone, Serialize)]
pub struct EffectiveConfig {
    pub ui: UiConfig,
    pub scan: ScanConfig,
    pub fix: FixConfig,
    pub privacy: PrivacyConfig,
    pub report: ReportConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UiConfig {
    pub color: bool,
    pub max_table_rows: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanConfig {
    pub default_scope: String,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FixConfig {
    pub default_risk_max: RiskLevel,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrivacyConfig {
    pub mask_home: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportConfig {
    pub include_evidence: bool,
}

impl Default for EffectiveConfig {
    fn default() -> Self {
        Self {
            ui: UiConfig {
                color: true,
                max_table_rows: 20,
            },
            scan: ScanConfig {
                default_scope: "dev".to_string(),
                exclude: vec!["**/node_modules/**".to_string()],
            },
            fix: FixConfig {
                default_risk_max: RiskLevel::R1,
            },
            privacy: PrivacyConfig { mask_home: true },
            report: ReportConfig {
                include_evidence: false,
            },
            config_path: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    ui: Option<RawUiConfig>,
    scan: Option<RawScanConfig>,
    fix: Option<RawFixConfig>,
    privacy: Option<RawPrivacyConfig>,
    report: Option<RawReportConfig>,
}

#[derive(Debug, Deserialize)]
struct RawUiConfig {
    color: Option<bool>,
    max_table_rows: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct RawScanConfig {
    default_scope: Option<String>,
    exclude: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RawFixConfig {
    default_risk_max: Option<RiskLevel>,
}

#[derive(Debug, Deserialize)]
struct RawPrivacyConfig {
    mask_home: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawReportConfig {
    include_evidence: Option<bool>,
}

pub fn default_config_path(home_dir: &Path) -> PathBuf {
    home_dir.join(".config/macdiet/config.toml")
}

pub fn load(config_path: Option<&Path>, home_dir: &Path) -> Result<EffectiveConfig> {
    let mut cfg = EffectiveConfig::default();

    let path = config_path
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| default_config_path(home_dir));

    if path.exists() {
        let s = std::fs::read_to_string(&path)
            .with_context(|| format!("設定ファイルの読み取りに失敗しました: {}", path.display()))?;
        let raw: RawConfig =
            toml::from_str(&s).context("設定ファイル(TOML)の解析に失敗しました")?;
        apply_raw_config(&mut cfg, raw);
        cfg.config_path = Some(path.display().to_string());
    }

    apply_env_overrides(&mut cfg)?;

    Ok(cfg)
}

fn apply_raw_config(cfg: &mut EffectiveConfig, raw: RawConfig) {
    if let Some(ui) = raw.ui {
        if let Some(color) = ui.color {
            cfg.ui.color = color;
        }
        if let Some(max_table_rows) = ui.max_table_rows {
            cfg.ui.max_table_rows = max_table_rows;
        }
    }

    if let Some(scan) = raw.scan {
        if let Some(default_scope) = scan.default_scope {
            cfg.scan.default_scope = default_scope;
        }
        if let Some(exclude) = scan.exclude {
            cfg.scan.exclude = exclude;
        }
    }

    if let Some(fix) = raw.fix {
        if let Some(default_risk_max) = fix.default_risk_max {
            cfg.fix.default_risk_max = default_risk_max;
        }
    }

    if let Some(privacy) = raw.privacy {
        if let Some(mask_home) = privacy.mask_home {
            cfg.privacy.mask_home = mask_home;
        }
    }

    if let Some(report) = raw.report {
        if let Some(include_evidence) = report.include_evidence {
            cfg.report.include_evidence = include_evidence;
        }
    }
}

fn apply_env_overrides(cfg: &mut EffectiveConfig) -> Result<()> {
    if let Ok(v) = std::env::var("MACDIET_UI_COLOR") {
        cfg.ui.color = parse_bool(&v).with_context(|| "MACDIET_UI_COLOR")?;
    }
    if let Ok(v) = std::env::var("MACDIET_UI_MAX_TABLE_ROWS") {
        cfg.ui.max_table_rows = v
            .trim()
            .parse::<usize>()
            .with_context(|| "MACDIET_UI_MAX_TABLE_ROWS")?;
    }
    if let Ok(v) = std::env::var("MACDIET_SCAN_DEFAULT_SCOPE") {
        let v = v.trim();
        if !v.is_empty() {
            cfg.scan.default_scope = v.to_string();
        }
    }
    if let Ok(v) = std::env::var("MACDIET_SCAN_EXCLUDE") {
        let parts: Vec<String> = v
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        if !parts.is_empty() {
            cfg.scan.exclude = parts;
        }
    }
    if let Ok(v) = std::env::var("MACDIET_FIX_DEFAULT_RISK_MAX") {
        cfg.fix.default_risk_max = v
            .parse::<RiskLevel>()
            .map_err(anyhow::Error::msg)
            .with_context(|| "MACDIET_FIX_DEFAULT_RISK_MAX")?;
    }
    if let Ok(v) = std::env::var("MACDIET_PRIVACY_MASK_HOME") {
        cfg.privacy.mask_home = parse_bool(&v).with_context(|| "MACDIET_PRIVACY_MASK_HOME")?;
    }
    if let Ok(v) = std::env::var("MACDIET_REPORT_INCLUDE_EVIDENCE") {
        cfg.report.include_evidence =
            parse_bool(&v).with_context(|| "MACDIET_REPORT_INCLUDE_EVIDENCE")?;
    }

    Ok(())
}

fn parse_bool(s: &str) -> Result<bool> {
    let s = s.trim().to_ascii_lowercase();
    match s.as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(anyhow::anyhow!(
            "真偽値が不正です: {s}（true|false|1|0|yes|no|on|off を指定してください）"
        )),
    }
}
