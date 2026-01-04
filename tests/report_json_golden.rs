use macdiet::core::{
    ActionKind, ActionPlan, ActionRef, Evidence, Finding, OsInfo, Report, ReportSummary, RiskLevel,
};

#[test]
fn report_json_matches_golden() {
    let report = Report {
        schema_version: "1.0".to_string(),
        tool_version: "0.1.0".to_string(),
        os: OsInfo {
            name: "macOS".to_string(),
            version: "26.x".to_string(),
        },
        generated_at: "2026-01-01T00:00:00Z".to_string(),
        summary: ReportSummary {
            estimated_total_bytes: 123,
            unobserved_bytes: 0,
            notes: vec!["note-1".to_string()],
        },
        findings: vec![Finding {
            id: "f-1".to_string(),
            finding_type: "XCODE_DERIVED_DATA_LARGE".to_string(),
            title: "Xcode DerivedData".to_string(),
            estimated_bytes: 123,
            confidence: 0.9,
            risk_level: RiskLevel::R1,
            evidence: vec![
                Evidence::path("~/Library/Developer/Xcode/DerivedData", true),
                Evidence::stat("files=1 errors=0"),
            ],
            recommended_actions: vec![ActionRef {
                id: "a-1".to_string(),
            }],
        }],
        actions: vec![ActionPlan {
            id: "a-1".to_string(),
            title: "Delete DerivedData via Xcode UI".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["f-1".to_string()],
            kind: ActionKind::ShowInstructions {
                markdown: "markdown".to_string(),
            },
            notes: vec![],
        }],
    };

    let actual = serde_json::to_value(&report).expect("serialize report");
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("golden/report.json")).expect("parse golden json");

    assert_eq!(actual, expected);
}
