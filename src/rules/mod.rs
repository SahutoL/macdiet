use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::core::{ActionKind, ActionPlan, ActionRef, Evidence, Finding, RiskLevel};
use crate::platform;
use crate::scan;

#[derive(Debug, Clone)]
pub struct RuleContext {
    pub home_dir: PathBuf,
    pub timeout: Duration,
    pub deadline: Option<Instant>,
    pub privacy_mask_home: bool,
}

impl RuleContext {
    pub fn command_timeout(&self) -> Duration {
        let Some(deadline) = self.deadline else {
            return self.timeout;
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        std::cmp::min(self.timeout, remaining)
    }
}

#[derive(Debug, Clone)]
pub struct RuleOutput {
    pub finding: Finding,
    pub actions: Vec<ActionPlan>,
}

pub fn doctor_rules(ctx: &RuleContext) -> Vec<RuleOutput> {
    let mut out = Vec::new();
    out.extend(xcode_derived_data(ctx));
    out.extend(coresimulator_devices(ctx));
    out.extend(xcode_archives(ctx));
    out.extend(xcode_device_support(ctx));
    out.extend(xcode_docsets(ctx));
    out.extend(xcode_device_logs(ctx));
    out.extend(docker_desktop_storage(ctx));
    out.extend(homebrew_cache(ctx));
    out.extend(cargo_registry_cache(ctx));
    out.extend(cargo_git_cache(ctx));
    out.extend(gradle_caches(ctx));
    out.extend(npm_cache(ctx));
    out.extend(yarn_cache(ctx));
    out.extend(pnpm_store_cache(ctx));
    out
}

pub fn snapshots_rules(ctx: &RuleContext) -> Vec<RuleOutput> {
    let mut out = Vec::new();
    out.push(tm_local_snapshots_status(ctx));
    out.push(apfs_snapshots_status(ctx));
    out
}

fn xcode_derived_data(ctx: &RuleContext) -> Option<RuleOutput> {
    let path = ctx.home_dir.join("Library/Developer/Xcode/DerivedData");
    let mut out = dir_finding(
        ctx,
        "xcode-derived-data",
        "XCODE_DERIVED_DATA_LARGE",
        "Xcode DerivedData（ビルドキャッシュ）",
        &path,
        RiskLevel::R1,
        Some(show_instructions_action(
            "xcode-derived-data-xcode-ui",
            "Xcode の UI から DerivedData を削除",
            RiskLevel::R1,
            vec!["xcode-derived-data".to_string()],
            r#"Xcode の UI から削除するのが安全です。

- Xcode を開く
- Settings(Preferences) → Locations
- "Derived Data" の行にある矢印/ボタンから削除

注意: DerivedData は再生成されます（次回ビルドが遅くなる可能性）。"#,
        )),
    )?;

    let action = ActionPlan {
        id: "xcode-derived-data-trash".to_string(),
        title: "DerivedData をゴミ箱へ移動（R1）".to_string(),
        risk_level: RiskLevel::R1,
        estimated_reclaimed_bytes: out.finding.estimated_bytes,
        related_findings: vec![out.finding.id.clone()],
        kind: ActionKind::TrashMove {
            paths: vec![maybe_mask_home(&path, &ctx.home_dir, true)],
        },
        notes: vec![
            "実行前に Xcode を終了してください。".to_string(),
            "影響: DerivedData は再生成されます（次回ビルドが遅くなる可能性があります）。"
                .to_string(),
        ],
    };
    out.finding.recommended_actions.push(ActionRef {
        id: action.id.clone(),
    });
    out.actions.push(action);
    Some(out)
}

fn coresimulator_devices(ctx: &RuleContext) -> Option<RuleOutput> {
    let path = ctx.home_dir.join("Library/Developer/CoreSimulator/Devices");
    let mut out = dir_finding(
        ctx,
        "coresimulator-devices",
        "CORESIMULATOR_DEVICES_LARGE",
        "CoreSimulator Devices（シミュレータデータ）",
        &path,
        RiskLevel::R2,
        Some(show_instructions_action(
            "coresimulator-devices-xcrun",
            "削除前に Simulator データを確認",
            RiskLevel::R2,
            vec!["coresimulator-devices".to_string()],
            r#"Simulator のデータは再生成可能ですが、状態が失われる可能性があります（R2）。

例:
- Xcode の Devices and Simulators で不要な Simulator を整理
- `xcrun simctl` の利用（削除は慎重に）"#,
        )),
    )?;

    let cmd = "xcrun simctl list devices unavailable";
    let cmd_timeout = ctx.command_timeout();
    let mut unavailable_present = None::<bool>;

    if cmd_timeout == Duration::from_secs(0) {
        out.finding.evidence.push(Evidence::command(cmd));
        out.finding.evidence.push(Evidence::stat(
            "xcrun simctl list devices unavailable: 未観測（タイムアウト予算消化）".to_string(),
        ));
    } else {
        match platform::run_command_invoking_user(
            "xcrun",
            &["simctl", "list", "devices", "unavailable"],
            cmd_timeout,
        ) {
            Ok(output) if output.exit_code == 0 => {
                unavailable_present = Some(simctl_list_has_unavailable(&output.stdout));
                out.finding.evidence.push(Evidence::command(cmd));
                out.finding.evidence.push(Evidence::stat(format!(
                    "xcrun simctl list devices unavailable: {}",
                    if unavailable_present == Some(true) {
                        "unavailable あり"
                    } else {
                        "unavailable なし"
                    }
                )));
            }
            Ok(output) => {
                out.finding.evidence.push(Evidence::command(cmd));
                out.finding.evidence.push(Evidence::stat(format!(
                    "xcrun simctl list devices unavailable: 未観測（exit_code={}）",
                    output.exit_code
                )));
            }
            Err(err) => {
                out.finding.evidence.push(Evidence::command(cmd));
                out.finding.evidence.push(Evidence::stat(format!(
                    "xcrun simctl list devices unavailable: 未観測（{err}）"
                )));
            }
        }
    }

    if unavailable_present != Some(false) {
        let action = ActionPlan {
            id: "coresimulator-simctl-delete-unavailable".to_string(),
            title: "利用できないシミュレータを削除（`xcrun simctl delete unavailable`）（R2）"
                .to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: out.finding.estimated_bytes,
            related_findings: vec![out.finding.id.clone()],
            kind: ActionKind::RunCmd {
                cmd: "xcrun".to_string(),
                args: vec![
                    "simctl".to_string(),
                    "delete".to_string(),
                    "unavailable".to_string(),
                ],
            },
            notes: vec![
                "影響: 利用できないデバイスを削除します。シミュレータの状態が変わる可能性があります。"
                    .to_string(),
                "注: これは CoreSimulator 全体の推定サイズです。`unavailable` の削除で必ず回収できるわけではありません。"
                    .to_string(),
                "ヒント: 事前に `xcrun simctl list devices unavailable` で確認してください。"
                    .to_string(),
            ],
        };

        out.finding.recommended_actions.push(ActionRef {
            id: action.id.clone(),
        });
        out.actions.push(action);
    }
    Some(out)
}

fn xcode_archives(ctx: &RuleContext) -> Option<RuleOutput> {
    let path = ctx.home_dir.join("Library/Developer/Xcode/Archives");
    dir_finding(
        ctx,
        "xcode-archives",
        "XCODE_ARCHIVES_LARGE",
        "Xcode Archives（アーカイブ）",
        &path,
        RiskLevel::R2,
        Some(show_instructions_action(
            "xcode-archives-review",
            "Xcode Organizer で古い Archives を確認",
            RiskLevel::R2,
            vec!["xcode-archives".to_string()],
            r#"Xcode Organizer の Archives から古いアーカイブを整理できます。

注意: 過去ビルドの配布・デバッグに必要な場合があります（R2）。"#,
        )),
    )
}

fn xcode_device_support(ctx: &RuleContext) -> Option<RuleOutput> {
    let path = ctx
        .home_dir
        .join("Library/Developer/Xcode/iOS DeviceSupport");
    dir_finding(
        ctx,
        "xcode-device-support",
        "DEVICE_SUPPORT_LARGE",
        "Xcode iOS DeviceSupport（デバッグ用データ）",
        &path,
        RiskLevel::R2,
        Some(show_instructions_action(
            "xcode-device-support-review",
            "古い DeviceSupport を確認",
            RiskLevel::R2,
            vec!["xcode-device-support".to_string()],
            r#"DeviceSupport は古い iOS バージョンのデバッグで使われる場合があります。

削除は慎重に（R2）。"#,
        )),
    )
}

fn xcode_docsets(ctx: &RuleContext) -> Option<RuleOutput> {
    let path = ctx
        .home_dir
        .join("Library/Developer/Shared/Documentation/DocSets");
    let mut out = dir_finding(
        ctx,
        "xcode-docsets",
        "XCODE_DOCSETS_LARGE",
        "Xcode DocSets（ドキュメント）",
        &path,
        RiskLevel::R1,
        Some(show_instructions_action(
            "xcode-docsets-review",
            "Xcode DocSets を確認",
            RiskLevel::R1,
            vec!["xcode-docsets".to_string()],
            r#"Xcode の DocSets はドキュメント（APIリファレンス等）で、再取得可能です（R1）。

注意:
- オフラインでドキュメントを参照したい場合は削除に注意してください"#,
        )),
    )?;

    let action = ActionPlan {
        id: "xcode-docsets-trash".to_string(),
        title: "Xcode DocSets をゴミ箱へ移動（R1）".to_string(),
        risk_level: RiskLevel::R1,
        estimated_reclaimed_bytes: out.finding.estimated_bytes,
        related_findings: vec![out.finding.id.clone()],
        kind: ActionKind::TrashMove {
            paths: vec![maybe_mask_home(&path, &ctx.home_dir, true)],
        },
        notes: vec![
            "影響: Xcode のドキュメントを再ダウンロードする必要が出ることがあります。".to_string(),
        ],
    };
    out.finding.recommended_actions.push(ActionRef {
        id: action.id.clone(),
    });
    out.actions.push(action);
    Some(out)
}

fn xcode_device_logs(ctx: &RuleContext) -> Option<RuleOutput> {
    let path = ctx.home_dir.join("Library/Developer/Xcode/iOS Device Logs");
    let mut out = dir_finding(
        ctx,
        "xcode-device-logs",
        "XCODE_DEVICE_LOGS_LARGE",
        "Xcode iOS Device Logs（端末ログ）",
        &path,
        RiskLevel::R1,
        Some(show_instructions_action(
            "xcode-device-logs-review",
            "iOS Device Logs を確認",
            RiskLevel::R1,
            vec!["xcode-device-logs".to_string()],
            r#"iOS Device Logs はデバイスログ/クラッシュログ等が溜まることがあります（R1）。

注意:
- 調査に必要なログが含まれている場合があります。削除前に確認してください"#,
        )),
    )?;

    let action = ActionPlan {
        id: "xcode-device-logs-trash".to_string(),
        title: "iOS Device Logs をゴミ箱へ移動（R1）".to_string(),
        risk_level: RiskLevel::R1,
        estimated_reclaimed_bytes: out.finding.estimated_bytes,
        related_findings: vec![out.finding.id.clone()],
        kind: ActionKind::TrashMove {
            paths: vec![maybe_mask_home(&path, &ctx.home_dir, true)],
        },
        notes: vec!["影響: 過去の端末ログ/クラッシュログが失われる可能性があります。".to_string()],
    };
    out.finding.recommended_actions.push(ActionRef {
        id: action.id.clone(),
    });
    out.actions.push(action);
    Some(out)
}

fn docker_desktop_storage(ctx: &RuleContext) -> Option<RuleOutput> {
    let path = ctx
        .home_dir
        .join("Library/Containers/com.docker.docker/Data");
    let mut out = dir_finding(
        ctx,
        "docker-desktop-data",
        "DOCKER_STORAGE_LARGE",
        "Docker Desktop Data（コンテナ/イメージ/キャッシュ）",
        &path,
        RiskLevel::R2,
        Some(ActionPlan {
            id: "docker-storage-df".to_string(),
            title: "Docker の使用量を確認（`docker system df`）".to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["docker-desktop-data".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "docker".to_string(),
                args: vec!["system".to_string(), "df".to_string()],
            },
            notes: vec![
                "注: これは読み取り専用の確認コマンドです（削除は行いません）。".to_string(),
                "ヒント: `docker system prune` は破壊的になり得るため慎重に。".to_string(),
            ],
        }),
    )?;

    let cmd = "docker system df";
    let cmd_timeout = ctx.command_timeout();
    if cmd_timeout == Duration::from_secs(0) {
        out.finding.evidence.push(Evidence::command(cmd));
        out.finding.evidence.push(Evidence::stat(
            "docker system df: 未観測（タイムアウト予算消化）".to_string(),
        ));
    } else {
        match platform::run_command_invoking_user("docker", &["system", "df"], cmd_timeout) {
            Ok(output) if output.exit_code == 0 => {
                out.finding.evidence.push(Evidence::command(cmd));
                out.finding
                    .evidence
                    .push(Evidence::stat(summarize_docker_system_df(&output.stdout)));
            }
            Ok(output) => {
                out.finding.evidence.push(Evidence::command(cmd));
                out.finding.evidence.push(Evidence::stat(format!(
                    "docker system df: 未観測（exit_code={}）",
                    output.exit_code
                )));
            }
            Err(err) => {
                out.finding.evidence.push(Evidence::command(cmd));
                out.finding
                    .evidence
                    .push(Evidence::stat(format!("docker system df: 未観測（{err}）")));
            }
        }
    }

    let builder_prune = ActionPlan {
        id: "docker-builder-prune".to_string(),
        title: "Docker build cache を prune（`docker builder prune`）（R2）".to_string(),
        risk_level: RiskLevel::R2,
        estimated_reclaimed_bytes: out.finding.estimated_bytes,
        related_findings: vec![out.finding.id.clone()],
        kind: ActionKind::RunCmd {
            cmd: "docker".to_string(),
            args: vec!["builder".to_string(), "prune".to_string()],
        },
        notes: vec![
            "影響: 未使用の build cache を削除します。次回ビルドが遅くなる可能性があります。"
                .to_string(),
            "ヒント: 対話的に実行してください（影響を理解していない限り `-f` は付けない）。"
                .to_string(),
        ],
    };
    out.finding.recommended_actions.push(ActionRef {
        id: builder_prune.id.clone(),
    });
    out.actions.push(builder_prune);

    let system_prune = ActionPlan {
        id: "docker-system-prune".to_string(),
        title: "Docker の未使用データを prune（`docker system prune`）（R2）".to_string(),
        risk_level: RiskLevel::R2,
        estimated_reclaimed_bytes: out.finding.estimated_bytes,
        related_findings: vec![out.finding.id.clone()],
        kind: ActionKind::RunCmd {
            cmd: "docker".to_string(),
            args: vec!["system".to_string(), "prune".to_string()],
        },
        notes: vec![
            "影響: 未使用のコンテナ/ネットワーク/イメージ(dangling)/build cache を削除します。"
                .to_string(),
            "ヒント: 何が削除されるか理解していない限り `--all` / `--volumes` は避けてください。"
                .to_string(),
        ],
    };
    out.finding.recommended_actions.push(ActionRef {
        id: system_prune.id.clone(),
    });
    out.actions.push(system_prune);

    Some(out)
}

fn gradle_caches(ctx: &RuleContext) -> Option<RuleOutput> {
    let path = ctx.home_dir.join(".gradle/caches");
    let mut out = dir_finding(
        ctx,
        "gradle-caches",
        "GRADLE_CACHES_LARGE",
        "Gradle caches（ビルドキャッシュ）",
        &path,
        RiskLevel::R1,
        Some(show_instructions_action(
            "gradle-caches-review",
            "Gradle caches を確認",
            RiskLevel::R1,
            vec!["gradle-caches".to_string()],
            r#"Gradle の caches は肥大化しやすく、再取得可能です（R1）。

ヒント:
- Android Studio/Gradle のビルド中は避ける
- 必要なら `./gradlew --stop` で daemon 停止を検討"#,
        )),
    )?;

    let action = ActionPlan {
        id: "gradle-caches-trash".to_string(),
        title: "Gradle caches をゴミ箱へ移動（R1）".to_string(),
        risk_level: RiskLevel::R1,
        estimated_reclaimed_bytes: out.finding.estimated_bytes,
        related_findings: vec![out.finding.id.clone()],
        kind: ActionKind::TrashMove {
            paths: vec![maybe_mask_home(&path, &ctx.home_dir, true)],
        },
        notes: vec![
            "影響: 依存関係を再ダウンロードする可能性があり、次回ビルドが遅くなることがあります。"
                .to_string(),
        ],
    };
    out.finding.recommended_actions.push(ActionRef {
        id: action.id.clone(),
    });
    out.actions.push(action);
    Some(out)
}

fn homebrew_cache(ctx: &RuleContext) -> Option<RuleOutput> {
    let path = ctx.home_dir.join("Library/Caches/Homebrew");
    let mut out = dir_finding(
        ctx,
        "homebrew-cache",
        "HOMEBREW_CACHE_LARGE",
        "Homebrew cache（キャッシュ）",
        &path,
        RiskLevel::R1,
        Some(ActionPlan {
            id: "homebrew-cache-cleanup".to_string(),
            title: "Homebrew cache を整理（`brew cleanup`）".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["homebrew-cache".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "brew".to_string(),
                args: vec!["cleanup".to_string()],
            },
            notes: vec![
                "注: `brew cleanup -s` はより積極的です。必要性を理解してから実行してください。"
                    .to_string(),
                "ヒント: Homebrew の操作中（install/upgrade 等）は避けてください。".to_string(),
            ],
        }),
    )?;

    let action = ActionPlan {
        id: "homebrew-cache-trash".to_string(),
        title: "Homebrew cache をゴミ箱へ移動（R1）".to_string(),
        risk_level: RiskLevel::R1,
        estimated_reclaimed_bytes: out.finding.estimated_bytes,
        related_findings: vec![out.finding.id.clone()],
        kind: ActionKind::TrashMove {
            paths: vec![maybe_mask_home(&path, &ctx.home_dir, true)],
        },
        notes: vec![
            "可能なら通常は `brew cleanup` の利用を推奨します。".to_string(),
            "影響: bottle の再ダウンロードが発生し、次回の install/upgrade が遅くなる可能性があります。"
                .to_string(),
        ],
    };
    out.finding.recommended_actions.push(ActionRef {
        id: action.id.clone(),
    });
    out.actions.push(action);
    Some(out)
}

fn cargo_registry_cache(ctx: &RuleContext) -> Option<RuleOutput> {
    let path = ctx.home_dir.join(".cargo/registry");
    let mut out = dir_finding(
        ctx,
        "cargo-registry",
        "RUST_CARGO_CACHE_LARGE",
        "Cargo registry cache（Rust）",
        &path,
        RiskLevel::R1,
        Some(show_instructions_action(
            "cargo-registry-review",
            "Cargo registry cache を確認",
            RiskLevel::R1,
            vec!["cargo-registry".to_string()],
            r#"Cargo の registry は再ダウンロード可能ですが、CI/オフライン作業に影響する場合があります（R1）。

まずはサイズ確認と内訳確認を推奨します。"#,
        )),
    )?;

    let action = ActionPlan {
        id: "cargo-registry-trash".to_string(),
        title: "Cargo registry cache をゴミ箱へ移動（R1）".to_string(),
        risk_level: RiskLevel::R1,
        estimated_reclaimed_bytes: out.finding.estimated_bytes,
        related_findings: vec![out.finding.id.clone()],
        kind: ActionKind::TrashMove {
            paths: vec![maybe_mask_home(&path, &ctx.home_dir, true)],
        },
        notes: vec![
            "影響: registry データを再ダウンロードします。オフライン作業や CI キャッシュに影響する可能性があります。"
                .to_string(),
        ],
    };
    out.finding.recommended_actions.push(ActionRef {
        id: action.id.clone(),
    });
    out.actions.push(action);
    Some(out)
}

fn cargo_git_cache(ctx: &RuleContext) -> Option<RuleOutput> {
    let path = ctx.home_dir.join(".cargo/git");
    let mut out = dir_finding(
        ctx,
        "cargo-git",
        "RUST_CARGO_CACHE_LARGE",
        "Cargo git cache（Rust）",
        &path,
        RiskLevel::R1,
        Some(show_instructions_action(
            "cargo-git-review",
            "Cargo git cache を確認",
            RiskLevel::R1,
            vec!["cargo-git".to_string()],
            r#"Cargo の git 依存キャッシュは再取得可能ですが、再ビルド時間が延びる可能性があります（R1）。"#,
        )),
    )?;

    let action = ActionPlan {
        id: "cargo-git-trash".to_string(),
        title: "Cargo git cache をゴミ箱へ移動（R1）".to_string(),
        risk_level: RiskLevel::R1,
        estimated_reclaimed_bytes: out.finding.estimated_bytes,
        related_findings: vec![out.finding.id.clone()],
        kind: ActionKind::TrashMove {
            paths: vec![maybe_mask_home(&path, &ctx.home_dir, true)],
        },
        notes: vec![
            "影響: git 依存を再取得する可能性があり、ビルドが遅くなることがあります。".to_string(),
        ],
    };
    out.finding.recommended_actions.push(ActionRef {
        id: action.id.clone(),
    });
    out.actions.push(action);
    Some(out)
}

fn npm_cache(ctx: &RuleContext) -> Option<RuleOutput> {
    let path = ctx.home_dir.join(".npm");
    let mut out = dir_finding(
        ctx,
        "npm-cache",
        "NODE_NPM_CACHE_LARGE",
        "npm cache（キャッシュ）",
        &path,
        RiskLevel::R1,
        Some(ActionPlan {
            id: "npm-cache-cleanup".to_string(),
            title: "npm cache を整理（`npm cache clean --force`）".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["npm-cache".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "npm".to_string(),
                args: vec![
                    "cache".to_string(),
                    "clean".to_string(),
                    "--force".to_string(),
                ],
            },
            notes: vec![
                "影響: npm のキャッシュを削除します。次回 `npm install` が遅くなる可能性があります。"
                    .to_string(),
                "注: `--force` の意味を理解してから実行してください。".to_string(),
            ],
        }),
    )?;

    let review = show_instructions_action(
        "npm-cache-review",
        "npm cache を確認",
        RiskLevel::R1,
        vec!["npm-cache".to_string()],
        r#"npm のキャッシュは再取得可能です（R1）。削除後は再インストールが遅くなる可能性があります。"#,
    );
    out.finding.recommended_actions.push(ActionRef {
        id: review.id.clone(),
    });
    out.actions.push(review);

    let action = ActionPlan {
        id: "npm-cache-trash".to_string(),
        title: "npm cache をゴミ箱へ移動（R1）".to_string(),
        risk_level: RiskLevel::R1,
        estimated_reclaimed_bytes: out.finding.estimated_bytes,
        related_findings: vec![out.finding.id.clone()],
        kind: ActionKind::TrashMove {
            paths: vec![maybe_mask_home(&path, &ctx.home_dir, true)],
        },
        notes: vec![
            "影響: パッケージの再ダウンロードにより、次回 `npm install` が遅くなる可能性があります。"
                .to_string(),
        ],
    };
    out.finding.recommended_actions.push(ActionRef {
        id: action.id.clone(),
    });
    out.actions.push(action);
    Some(out)
}

fn yarn_cache(ctx: &RuleContext) -> Option<RuleOutput> {
    let path = ctx.home_dir.join("Library/Caches/Yarn");
    let mut out = dir_finding(
        ctx,
        "yarn-cache",
        "NODE_YARN_CACHE_LARGE",
        "Yarn cache（キャッシュ）",
        &path,
        RiskLevel::R1,
        Some(ActionPlan {
            id: "yarn-cache-cleanup".to_string(),
            title: "Yarn cache を整理（`yarn cache clean`）".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["yarn-cache".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "yarn".to_string(),
                args: vec!["cache".to_string(), "clean".to_string()],
            },
            notes: vec![
                "影響: Yarn のキャッシュを削除します。次回 `yarn install` が遅くなる可能性があります。"
                    .to_string(),
                "注: yarn の実行中（install 等）は避けてください。".to_string(),
            ],
        }),
    )?;

    let review = show_instructions_action(
        "yarn-cache-review",
        "Yarn cache を確認",
        RiskLevel::R1,
        vec!["yarn-cache".to_string()],
        r#"Yarn のキャッシュは再取得可能です（R1）。"#,
    );
    out.finding.recommended_actions.push(ActionRef {
        id: review.id.clone(),
    });
    out.actions.push(review);

    let action = ActionPlan {
        id: "yarn-cache-trash".to_string(),
        title: "Yarn cache をゴミ箱へ移動（R1）".to_string(),
        risk_level: RiskLevel::R1,
        estimated_reclaimed_bytes: out.finding.estimated_bytes,
        related_findings: vec![out.finding.id.clone()],
        kind: ActionKind::TrashMove {
            paths: vec![maybe_mask_home(&path, &ctx.home_dir, true)],
        },
        notes: vec![
            "影響: パッケージの再ダウンロードにより、次回 `yarn install` が遅くなる可能性があります。"
                .to_string(),
        ],
    };
    out.finding.recommended_actions.push(ActionRef {
        id: action.id.clone(),
    });
    out.actions.push(action);
    Some(out)
}

fn pnpm_store_cache(ctx: &RuleContext) -> Option<RuleOutput> {
    // pnpm の store は環境により場所が異なるため、代表的な2箇所を候補にする。
    let candidates = [
        ctx.home_dir.join(".pnpm-store"),
        ctx.home_dir.join("Library/pnpm/store"),
    ];

    for path in candidates {
        if let Some(mut found) = dir_finding(
            ctx,
            "pnpm-store",
            "NODE_PNPM_STORE_LARGE",
            "pnpm store（キャッシュ）",
            &path,
            RiskLevel::R1,
            Some(ActionPlan {
                id: "pnpm-store-prune".to_string(),
                title: "pnpm store を整理（`pnpm store prune`）".to_string(),
                risk_level: RiskLevel::R1,
                estimated_reclaimed_bytes: 0,
                related_findings: vec!["pnpm-store".to_string()],
                kind: ActionKind::RunCmd {
                    cmd: "pnpm".to_string(),
                    args: vec!["store".to_string(), "prune".to_string()],
                },
                notes: vec![
                    "影響: 未使用の store データを削除します。次回 `pnpm install` が遅くなる可能性があります。"
                        .to_string(),
                    "注: pnpm の実行中（install 等）は避けてください。".to_string(),
                ],
            }),
        ) {
            let review = show_instructions_action(
                "pnpm-store-review",
                "pnpm store を確認",
                RiskLevel::R1,
                vec!["pnpm-store".to_string()],
                r#"pnpm store は再取得可能ですが、削除は次回インストール時間に影響します（R1）。"#,
            );
            found.finding.recommended_actions.push(ActionRef {
                id: review.id.clone(),
            });
            found.actions.push(review);

            let action = ActionPlan {
                id: "pnpm-store-trash".to_string(),
                title: "pnpm store をゴミ箱へ移動（R1）".to_string(),
                risk_level: RiskLevel::R1,
                estimated_reclaimed_bytes: found.finding.estimated_bytes,
                related_findings: vec![found.finding.id.clone()],
                kind: ActionKind::TrashMove {
                    paths: vec![maybe_mask_home(&path, &ctx.home_dir, true)],
                },
                notes: vec![
                    "影響: パッケージの再ダウンロードにより、次回 `pnpm install` が遅くなる可能性があります。"
                        .to_string(),
                ],
            };
            found.finding.recommended_actions.push(ActionRef {
                id: action.id.clone(),
            });
            found.actions.push(action);
            return Some(found);
        }
    }

    None
}

fn tm_local_snapshots_status(ctx: &RuleContext) -> RuleOutput {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = ctx;
        let finding = Finding {
            id: "tm-local-snapshots-unavailable".to_string(),
            finding_type: "TM_LOCAL_SNAPSHOTS_UNOBSERVED".to_string(),
            title: "Time Machine ローカルスナップショット: このOSでは状態を取得できません"
                .to_string(),
            estimated_bytes: 0,
            confidence: 0.0,
            risk_level: RiskLevel::R0,
            evidence: vec![],
            recommended_actions: vec![],
        };
        return RuleOutput {
            finding,
            actions: vec![],
        };
    }

    #[cfg(target_os = "macos")]
    {
        let cmd = "tmutil listlocalsnapshots /";
        let cmd_timeout = ctx.command_timeout();
        if cmd_timeout == Duration::from_secs(0) {
            let action = tmutil_unobserved_action();
            let finding = Finding {
                id: "tm-local-snapshots-unobserved".to_string(),
                finding_type: "TM_LOCAL_SNAPSHOTS_UNOBSERVED".to_string(),
                title: "Time Machine ローカルスナップショット: 未観測（タイムアウト予算消化）"
                    .to_string(),
                estimated_bytes: 0,
                confidence: 0.3,
                risk_level: RiskLevel::R0,
                evidence: vec![
                    Evidence::command(cmd),
                    Evidence::stat("タイムアウト予算消化".to_string()),
                ],
                recommended_actions: vec![ActionRef {
                    id: action.id.clone(),
                }],
            };
            return RuleOutput {
                finding,
                actions: vec![action],
            };
        }

        let output = crate::platform::macos::tmutil_list_local_snapshots(cmd_timeout);
        let output = match output {
            Ok(output) => output,
            Err(err) => {
                let action = tmutil_unobserved_action();
                let finding = Finding {
                    id: "tm-local-snapshots-unobserved".to_string(),
                    finding_type: "TM_LOCAL_SNAPSHOTS_UNOBSERVED".to_string(),
                    title: "Time Machine ローカルスナップショット: 未観測（tmutil が失敗）"
                        .to_string(),
                    estimated_bytes: 0,
                    confidence: 0.3,
                    risk_level: RiskLevel::R0,
                    evidence: vec![Evidence::command(cmd), Evidence::stat(err.to_string())],
                    recommended_actions: vec![ActionRef {
                        id: action.id.clone(),
                    }],
                };
                return RuleOutput {
                    finding,
                    actions: vec![action],
                };
            }
        };

        if output.exit_code != 0 {
            let mut evidence = vec![Evidence::command(cmd)];
            if !output.stderr.trim().is_empty() {
                evidence.push(Evidence::stat(output.stderr.trim().to_string()));
            }
            evidence.push(Evidence::stat(format!("exit_code={}", output.exit_code)));

            let action = tmutil_unobserved_action();
            let finding = Finding {
                id: "tm-local-snapshots-unobserved".to_string(),
                finding_type: "TM_LOCAL_SNAPSHOTS_UNOBSERVED".to_string(),
                title: "Time Machine ローカルスナップショット: 未観測（tmutil が非0で終了）"
                    .to_string(),
                estimated_bytes: 0,
                confidence: 0.3,
                risk_level: RiskLevel::R0,
                evidence,
                recommended_actions: vec![ActionRef {
                    id: action.id.clone(),
                }],
            };
            return RuleOutput {
                finding,
                actions: vec![action],
            };
        }

        let count = parse_tmutil_local_snapshot_count(&output.stdout);

        if count == 0 {
            let finding = Finding {
                id: "tm-local-snapshots-none".to_string(),
                finding_type: "TM_LOCAL_SNAPSHOTS_NONE".to_string(),
                title: "Time Machine ローカルスナップショット: なし".to_string(),
                estimated_bytes: 0,
                confidence: 0.8,
                risk_level: RiskLevel::R0,
                evidence: vec![Evidence::command(cmd), Evidence::stat("count=0")],
                recommended_actions: vec![],
            };
            return RuleOutput {
                finding,
                actions: vec![],
            };
        }

        let finding_id = "tm-local-snapshots-present".to_string();
        let action_id = "tm-local-snapshots-thin".to_string();

        let finding = Finding {
            id: finding_id.clone(),
            finding_type: "TM_LOCAL_SNAPSHOTS_PRESENT".to_string(),
            title: format!("Time Machine ローカルスナップショットを検出（count: {count}）"),
            estimated_bytes: 0,
            confidence: 0.8,
            risk_level: RiskLevel::R3,
            evidence: vec![
                Evidence::command(cmd),
                Evidence::stat(format!("count={count}")),
            ],
            recommended_actions: vec![ActionRef {
                id: action_id.clone(),
            }],
        };

        let actions = vec![ActionPlan {
            id: action_id,
            title: "ローカルスナップショットの thin を検討（R3）".to_string(),
            risk_level: RiskLevel::R3,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![finding_id],
            kind: ActionKind::ShowInstructions {
                markdown: r#"Apple はローカルスナップショットは容量が必要な場合などに自動削除されると説明しています。

それでも逼迫している場合は、手動で「thin（薄め）」を検討できます（R3）。

例:
- `tmutil thinlocalsnapshots / <bytes> <urgency:1..4>`

注意:
- 実行には sudo が必要になる場合があります
- 影響が大きい可能性があるため、実行前に十分確認してください"#.to_string(),
            },
            notes: vec![],
        }];

        RuleOutput { finding, actions }
    }
}

fn apfs_snapshots_status(ctx: &RuleContext) -> RuleOutput {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = ctx;
        let finding = Finding {
            id: "apfs-snapshots-unavailable".to_string(),
            finding_type: "APFS_SNAPSHOTS_UNOBSERVED".to_string(),
            title: "APFS スナップショット: このOSでは状態を取得できません".to_string(),
            estimated_bytes: 0,
            confidence: 0.0,
            risk_level: RiskLevel::R0,
            evidence: vec![],
            recommended_actions: vec![],
        };
        return RuleOutput {
            finding,
            actions: vec![],
        };
    }

    #[cfg(target_os = "macos")]
    {
        let cmd = "diskutil apfs listSnapshots /";
        let cmd_timeout = ctx.command_timeout();
        if cmd_timeout == Duration::from_secs(0) {
            let mut action = show_apfs_disk_utility_action();
            action.related_findings = vec!["apfs-snapshots-unobserved".to_string()];
            let finding = Finding {
                id: "apfs-snapshots-unobserved".to_string(),
                finding_type: "APFS_SNAPSHOTS_UNOBSERVED".to_string(),
                title: "APFS スナップショット: 未観測（タイムアウト予算消化）".to_string(),
                estimated_bytes: 0,
                confidence: 0.3,
                risk_level: RiskLevel::R0,
                evidence: vec![
                    Evidence::command(cmd),
                    Evidence::stat("タイムアウト予算消化".to_string()),
                ],
                recommended_actions: vec![ActionRef {
                    id: action.id.clone(),
                }],
            };
            return RuleOutput {
                finding,
                actions: vec![action],
            };
        }

        let output = crate::platform::macos::diskutil_apfs_list_snapshots("/", cmd_timeout);
        let output = match output {
            Ok(output) => output,
            Err(err) => {
                let mut action = show_apfs_disk_utility_action();
                action.related_findings = vec!["apfs-snapshots-unobserved".to_string()];
                let finding = Finding {
                    id: "apfs-snapshots-unobserved".to_string(),
                    finding_type: "APFS_SNAPSHOTS_UNOBSERVED".to_string(),
                    title: "APFS スナップショット: 未観測（diskutil が失敗）".to_string(),
                    estimated_bytes: 0,
                    confidence: 0.3,
                    risk_level: RiskLevel::R0,
                    evidence: vec![Evidence::command(cmd), Evidence::stat(err.to_string())],
                    recommended_actions: vec![ActionRef {
                        id: action.id.clone(),
                    }],
                };
                return RuleOutput {
                    finding,
                    actions: vec![action],
                };
            }
        };

        if output.exit_code != 0 {
            let mut evidence = vec![Evidence::command(cmd)];
            if !output.stderr.trim().is_empty() {
                evidence.push(Evidence::stat(output.stderr.trim().to_string()));
            }
            evidence.push(Evidence::stat(format!("exit_code={}", output.exit_code)));

            let mut action = show_apfs_disk_utility_action();
            action.related_findings = vec!["apfs-snapshots-unobserved".to_string()];
            let finding = Finding {
                id: "apfs-snapshots-unobserved".to_string(),
                finding_type: "APFS_SNAPSHOTS_UNOBSERVED".to_string(),
                title: "APFS スナップショット: 未観測（diskutil が非0で終了）".to_string(),
                estimated_bytes: 0,
                confidence: 0.3,
                risk_level: RiskLevel::R0,
                evidence,
                recommended_actions: vec![ActionRef {
                    id: action.id.clone(),
                }],
            };
            return RuleOutput {
                finding,
                actions: vec![action],
            };
        }

        let count = parse_diskutil_snapshot_count(&output.stdout).unwrap_or_else(|| {
            output
                .stdout
                .lines()
                .filter(|l| l.trim_start().starts_with("+--"))
                .count()
        });

        if count == 0 {
            let finding = Finding {
                id: "apfs-snapshots-none".to_string(),
                finding_type: "APFS_SNAPSHOTS_NONE".to_string(),
                title: "APFS スナップショット: なし（diskutil）".to_string(),
                estimated_bytes: 0,
                confidence: 0.8,
                risk_level: RiskLevel::R0,
                evidence: vec![Evidence::command(cmd), Evidence::stat("count=0")],
                recommended_actions: vec![],
            };
            return RuleOutput {
                finding,
                actions: vec![],
            };
        }

        let action_id = "apfs-snapshots-disk-utility".to_string();
        let finding = Finding {
            id: "apfs-snapshots-present".to_string(),
            finding_type: "APFS_SNAPSHOTS_PRESENT".to_string(),
            title: format!("APFS スナップショットを検出（count: {count}）"),
            estimated_bytes: 0,
            confidence: 0.8,
            risk_level: RiskLevel::R3,
            evidence: vec![
                Evidence::command(cmd),
                Evidence::stat(format!("count={count}")),
            ],
            recommended_actions: vec![ActionRef {
                id: action_id.clone(),
            }],
        };

        let mut action = show_apfs_disk_utility_action();
        action.id = action_id;
        action.related_findings = vec!["apfs-snapshots-present".to_string()];
        RuleOutput {
            finding,
            actions: vec![action],
        }
    }
}

fn show_apfs_disk_utility_action() -> ActionPlan {
    ActionPlan {
        id: "apfs-snapshots-disk-utility".to_string(),
        title: "Disk Utility で APFS スナップショットを確認（R3）".to_string(),
        risk_level: RiskLevel::R3,
        estimated_reclaimed_bytes: 0,
        related_findings: vec![],
        kind: ActionKind::ShowInstructions {
            markdown: r#"APFS スナップショットは Disk Utility で閲覧/削除できます（R3）。

もし `diskutil` が失敗して「未観測」になった場合:
- macdiet を実行しているターミナルに Full Disk Access を許可して再実行
- `sudo diskutil apfs listSnapshots /` を試す（必要な場合があります）
- それでも失敗する場合は Disk Utility を優先（環境によって `diskutil` が利用できないことがあります）

導線（例）:
- Disk Utility を開く
- 対象ボリュームを選択
- Snapshots（スナップショット）を確認

注意:
- 削除は影響が大きい可能性があるため、実行前に十分確認してください
- `diskutil` のCLI操作は環境差があり得るため、ツール側で検出したIDのみ許可する方針です"#
                .to_string(),
        },
        notes: vec![],
    }
}

fn tmutil_unobserved_action() -> ActionPlan {
    show_instructions_action(
        "tm-local-snapshots-troubleshoot",
        "Time Machine ローカルスナップショットのトラブルシュート（tmutil）",
        RiskLevel::R0,
        vec!["tm-local-snapshots-unobserved".to_string()],
        r#"Time Machine ローカルスナップショットの検出に失敗しました（未観測）。

確認手順（例）:
- `tmutil listlocalsnapshots /` を手動で実行
- macdiet を実行しているターミナルに Full Disk Access を許可して再実行
- 必要なら `sudo tmutil listlocalsnapshots /` を試す（環境により必要）

補足:
- ローカルスナップショットは容量が必要な場合などに自動削除されることがあります（Apple の説明に従う）。"#,
    )
}

fn parse_diskutil_snapshot_count(stdout: &str) -> Option<usize> {
    for line in stdout.lines() {
        let line = line.trim();
        if !line.contains("found") {
            continue;
        }
        let open = line.find('(')?;
        let close = line[open..].find(')')? + open;
        let inside = &line[open + 1..close];
        let digits: String = inside.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            continue;
        }
        if !inside.contains("found") {
            continue;
        }
        return digits.parse::<usize>().ok();
    }
    None
}

fn parse_tmutil_local_snapshot_count(stdout: &str) -> usize {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| !l.starts_with("Snapshots for disk"))
        .count()
}

fn simctl_list_has_unavailable(stdout: &str) -> bool {
    stdout.lines().any(|line| {
        let line = line.trim();
        if line.is_empty() {
            return false;
        }
        line.contains("unavailable") && line.contains('(')
    })
}

fn summarize_docker_system_df(stdout: &str) -> String {
    let mut kept = Vec::new();
    for line in stdout.lines().map(str::trim_end) {
        let t = line.trim_start();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("TYPE")
            || t.starts_with("Images")
            || t.starts_with("Containers")
            || t.starts_with("Local Volumes")
            || t.starts_with("Build Cache")
        {
            kept.push(t.to_string());
        }
    }

    if kept.is_empty() {
        for line in stdout
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.trim().is_empty())
        {
            kept.push(line.trim_start().to_string());
            if kept.len() >= 8 {
                break;
            }
        }
    }

    if kept.is_empty() {
        return "docker system df: 出力が空でした".to_string();
    }

    format!("docker system df:\n{}", kept.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tmutil_header_only_is_zero() {
        let stdout = "Snapshots for disk /:\n";
        assert_eq!(parse_tmutil_local_snapshot_count(stdout), 0);
    }

    #[test]
    fn parse_tmutil_counts_snapshots() {
        let stdout = "Snapshots for disk /:\ncom.apple.TimeMachine.2026-01-01-000000.local\ncom.apple.TimeMachine.2026-01-02-000000.local\n";
        assert_eq!(parse_tmutil_local_snapshot_count(stdout), 2);
    }

    #[test]
    fn parse_diskutil_snapshot_count_from_found_line() {
        let stdout = "Snapshots for disk1s1 (12 found)\n|\n+-- ABC\n";
        assert_eq!(parse_diskutil_snapshot_count(stdout), Some(12));
    }

    #[test]
    fn simctl_list_has_unavailable_detects_device_lines() {
        let stdout = "== Devices ==\n-- iOS 17.0 --\n    iPhone 14 (AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE) (Shutdown) (unavailable, runtime profile not found)\n";
        assert!(simctl_list_has_unavailable(stdout));
    }

    #[test]
    fn simctl_list_has_unavailable_is_false_for_empty_or_headers() {
        assert!(!simctl_list_has_unavailable(""));
        assert!(!simctl_list_has_unavailable("== Devices ==\n"));
        assert!(!simctl_list_has_unavailable("-- iOS 17.0 --\n"));
    }
}

fn dir_finding(
    ctx: &RuleContext,
    finding_id: &str,
    finding_type: &str,
    title: &str,
    path: &Path,
    risk_level: RiskLevel,
    action: Option<ActionPlan>,
) -> Option<RuleOutput> {
    if !path.exists() {
        return None;
    }

    let estimate = scan::estimate_dir_size(path, ctx.timeout, ctx.deadline).ok()?;
    if estimate.bytes == 0 {
        return None;
    }

    let masked_path = maybe_mask_home(path, &ctx.home_dir, ctx.privacy_mask_home);
    let evidence = vec![
        Evidence::path(masked_path, ctx.privacy_mask_home),
        Evidence::stat(format!(
            "files={} errors={} method={:?}",
            estimate.file_count, estimate.error_count, estimate.method
        )),
    ];

    let mut actions = Vec::new();
    let mut action_refs = Vec::new();
    if let Some(mut action) = action {
        if action.estimated_reclaimed_bytes == 0 {
            action.estimated_reclaimed_bytes = estimate.bytes;
        }
        action_refs.push(ActionRef {
            id: action.id.clone(),
        });
        actions.push(action);
    }

    let finding = Finding {
        id: finding_id.to_string(),
        finding_type: finding_type.to_string(),
        title: format!("{title}: {}", maybe_mask_home(path, &ctx.home_dir, true)),
        estimated_bytes: estimate.bytes,
        confidence: estimate.confidence(),
        risk_level,
        evidence,
        recommended_actions: action_refs,
    };

    Some(RuleOutput { finding, actions })
}

fn show_instructions_action(
    id: &str,
    title: &str,
    risk_level: RiskLevel,
    related_findings: Vec<String>,
    markdown: &str,
) -> ActionPlan {
    ActionPlan {
        id: id.to_string(),
        title: title.to_string(),
        risk_level,
        estimated_reclaimed_bytes: 0,
        related_findings,
        kind: ActionKind::ShowInstructions {
            markdown: markdown.to_string(),
        },
        notes: vec![],
    }
}

fn maybe_mask_home(path: &Path, home_dir: &Path, mask_home: bool) -> String {
    if !mask_home {
        return path.display().to_string();
    }

    let Ok(stripped) = path.strip_prefix(home_dir) else {
        return path.display().to_string();
    };
    let stripped = stripped.display().to_string();
    if stripped.is_empty() {
        "~".to_string()
    } else {
        format!("~/{stripped}")
    }
}
