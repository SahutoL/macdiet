# macdiet: review

## レビュー方針

- 一次仕様は `仕様書.md`（FR/安全モデル/UX要件/MVP条件）
- レビューの目的は「仕様適合・安全・UX・品質」の継続点検と、次の作業優先度を明確化すること

## レビュー履歴

### 2026-01-04: 最小一区切り（TUI Phase 10）

- 実装: TUIから `snapshots thin/delete`（R3）を実行（最大リスク=R3ゲート + 二段階typed confirm + 結果/ログ表示）
- UI/UX: ホームに `スナップショット thin/delete` を追加、ヘルプ/ドキュメント/READMEを追従
- 安全: 既存方針を維持（ツール側の勝手なsudo昇格なし、ログ保存、`--dry-run` で破壊的操作無効）

### 2026-01-02: 初期セットアップ

- 実施: エージェント運用ルールの整備（`AGENTS.md` / `rules.md`）、進捗管理テンプレ（`now-task.md` / `review.md`）
- 所見: 実装は未着手。次はRust雛形作成→CLI骨格→ドメインモデル→検出ルールの順でMVP(v0.1)へ寄せる。

### 2026-01-02: 実装の初期進捗（MVP土台）

- 実装: Rustプロジェクト初期化（`Cargo.toml`/`src/`）、CLI骨格（`doctor/scan/snapshots/fix/report`）、固定スキーマJSON（golden test）
- 検出: Xcode/Simulator/Docker/Homebrew/Cargo/Node系の代表パスをdoctorで推定（サイズ+根拠パス）
- scan: `scan --deep` の初期版（scope preset/path、トップディレクトリランキング、exclude glob）
- 未完: `fix` の安全実行（R1 dry-run→apply）、`snapshots` のdiskutil/APFS側、UX（進捗・未観測バイトの扱い）、統合/セーフティテスト拡充

### 2026-01-02: snapshots/fix の前進（安全モデル寄せ）

- snapshots: `snapshots status` で tmutil / diskutil を試行し、失敗時は「未観測」として可視化（Disk Utility導線を提示）
- fix: R1のみの dry-run を実装（TRASH_MOVE候補+SHOW_INSTRUCTIONS、ホワイトリスト検証を追加、`--apply` は未対応で変更ゼロを保証）

### 2026-01-02: 限定実行の追加（R1のみ）

- fix: `fix --apply` をR1/TRASH_MOVE限定で実装（TTY+明示確認、`~/.Trash` へ移動、ホワイトリスト外は拒否）

### 2026-01-02: doctor/scan のUX改善

- doctor: 進捗（stderr spinner）、色（TTY時のRisk色分け）、未観測の注意喚起、エラーブロック整形
- scan: `scan --deep` の進捗（stderr spinner）を追加（`--json`/非TTY/`--quiet`で抑制）

### 2026-01-02: report の整備

- report: `report --json` をサポート（global化）、evidenceは既定で非表示・`--include-evidence`で表示（JSON/markdown両方）

### 2026-01-02: セーフティテスト追加

- CLI: non-TTY では `fix --apply` を拒否、dry-run は変更ゼロ、report のevidence既定非表示を統合テストで確認（`tests/cli_safety.rs`）

### 2026-01-02: config 対応

- TOML（`~/.config/macdiet/config.toml`）を読み込み、UI/scan/fix/reportの既定値に反映（`macdiet config --show` で有効値を表示）

### 2026-01-02: completion 対応

- `macdiet completion bash|zsh|fish` を追加（clap_complete）

### 2026-01-02: env override 対応

- config: `MACDIET_*` による上書きを追加（default < config < env < CLI の優先順位）
- tests: env優先順位の統合テストを追加（`tests/env_precedence.rs`）

### 2026-01-02: fix --interactive 対応

- fix: `--interactive` で候補Actionを番号選択し、dry-run/`--apply`（R1/TRASH_MOVEのみ）に反映
- safety: `--apply` の確認を2段階化（`yes` → `trash`）
- tests: 選択文字列パーサのユニットテストを追加（`src/cli/interactive.rs`）

### 2026-01-02: 終了コードの整備

- exit: invalid args=2 / 致命=10 / 外部コマンド=20 の枠を追加し、Usage系エラーを2で返す（`src/exit.rs`）
- tests: 代表的なUsageエラーが2で返ることを統合テストで担保（`tests/exit_codes.rs`）

### 2026-01-02: snapshots thin 対応（R3）

- snapshots: `snapshots thin --bytes <N> --urgency <1..4>` を実装（TTY+二段階確認、`--dry-run` で非破壊プレビュー）
- exit: tmutil失敗/非ゼロ終了を外部コマンド失敗（exit=20）として扱う

### 2026-01-02: snapshots delete 対応（R3）

- snapshots: `snapshots delete --id <uuid>` を実装（TTY+二段階確認、`--dry-run` 対応、`diskutil apfs listSnapshots /` で検出したUUIDのみ受理）
- exit: diskutil失敗/非ゼロ終了を外部コマンド失敗（exit=20）として扱う
- tests: 非TTYでは拒否（exit=2）を統合テストで担保（`tests/exit_codes.rs`）

### 2026-01-02: ドキュメント追従

- README/docs/SECURITY を現状の機能に追従（`fix --interactive` / `snapshots thin/delete` / config/env / exit codes）

### 2026-01-02: snapshots ID検出の改善

- snapshots: `diskutil apfs listSnapshots` 出力から Name/UUID を抽出し、`snapshots delete --id <uuid|name>` を安全に解決（曖昧な場合は拒否）
- docs/tests: snapshots docs を更新し、パーサのユニットテストを追加（`src/snapshots/mod.rs`）

### 2026-01-02: fix UX改善（影響表示）

- fix: TRASH_MOVEのImpactをnotesとして付与し、`fix` 出力で常に表示（実行前の影響説明を強化）
- fix: SHOW_INSTRUCTIONS を要約表示（先頭行＋verbose時の一部展開）
- tests: fix出力にImpact noteが含まれることを統合テストで担保（`tests/cli_safety.rs`）

### 2026-01-02: exit=20の方針整理

- 外部コマンドを直接実行する操作（thin/delete等）は exit=20 で統一し、診断系（doctor/scan）は継続可能なら未観測/警告に落とす方針を明確化

### 2026-01-02: doctor検出カテゴリ拡充

- rules: Xcode DocSets / iOS Device Logs / Gradle caches を追加し、R1のTRASH_MOVEと影響ノートを整備（`src/rules/mod.rs`）
- safety: TRASH_MOVEホワイトリストを拡張（`src/actions/mod.rs`）
- tests: Gradle caches をdoctorが検出できることを統合テストで担保（`tests/cli_safety.rs`）

### 2026-01-02: Docker検出の強化

- rules: `docker system df` を併用して根拠（Evidence）にサマリを追加（失敗時はunobservedとして扱い、ファイル推定にフォールバック）
- tests: 疑似dockerコマンドでEvidenceが追加されることを統合テストで担保（`tests/cli_safety.rs`）

### 2026-01-02: scan devスコープ拡充

- scan: devスコープに`~/.gradle`を追加（`src/engine.rs`）
- docs/tests: READMEにscope presetsを追記し、`scan --deep --scope dev`で`.gradle`が拾えることを統合テストで担保（`tests/cli_safety.rs`）

### 2026-01-02: doctor表示の一貫性改善

- ui: Top Findings / Recommended Actions の表示件数を明示し、表示対象の整合を改善（`src/ui/mod.rs`）
- docs/tests: `ui.max_table_rows` の説明を追記し、ヘッダが表示件数に追従することを統合テストで担保（`tests/cli_safety.rs`）

### 2026-01-02: fix --target UX改善

- ui: fix出力にAction ID/targets（Finding ID）を表示し、対象指定をしやすく改善（`src/ui/mod.rs`）
- cli: 未知の`--target`を検出してexit=2でヒントを提示（`src/cli/mod.rs`）
- docs/tests: READMEに`--target`例を追記し、未知ターゲット時の挙動を統合テストで担保（`tests/cli_safety.rs`）

### 2026-01-02: report --markdown の整形強化

- markdown: Findings/Actions を見出し化し、id/risk/targets/paths/impact を読みやすく整形（`src/cli/mod.rs`）
- instructions: SHOW_INSTRUCTIONS はMarkdownとしてそのまま展開して貼り付け可能に（`src/cli/mod.rs`）
- evidence: `--include-evidence` 時のみ出力し、複数行のstatは fenced code block で可視化（`src/cli/mod.rs`）
- tests: `report --markdown` の体裁・evidenceの有無・複数行evidenceの整形を統合テストで担保（`tests/report_markdown.rs`）

### 2026-01-02: fix のR2プレビュー強化

- fix: R2+ を含む場合は「TRASH_MOVEでの削減見込み」と「R2+の見込み（プレビュー）」を分けて表示（`src/ui/mod.rs`）
- fix: SHOW_INSTRUCTIONS の要約を改善し、R2+は注意/慎重等の行を note として補足（`src/ui/mod.rs`）
- tests: R2の注意行がfix出力に出ることを統合テストで担保（`tests/fix_r2_preview.rs`）

### 2026-01-02: fix のR2提案追加（プレビューのみ）

- rules: CoreSimulator / Docker のR2改善案として RUN_CMD アクションを追加（`xcrun simctl delete unavailable` / `docker builder prune` / `docker system prune`）（`src/rules/mod.rs`）
- ui: RUN_CMD の表示をコマンドライン形式に整形（`src/ui/mod.rs`）
- docs/tests: `fix --risk R2` にコマンド提案が出ることを統合テストで担保し、READMEに「R2+は提案のみ」を追記（`tests/fix_r2_preview.rs`, `README.md`）

### 2026-01-02: doctor のSystem Data解説強化

- doctor: Summary notes を Apple 定義に寄せて明確化し、「再分類して提示する」方針を追記（`src/engine.rs`）
- unobserved: Full Disk Access の導線を具体化（システム設定のパスを提示）し、`docs/system-data.md` にも追記（`src/engine.rs`, `docs/system-data.md`）
- tests: doctor出力にSystem Data定義が含まれること／未観測時に権限導線が出ることを統合テストで担保（`tests/cli_safety.rs`）

### 2026-01-02: doctor の未観測バイト推定改善

- engine: `errors=` を元に `summary.unobserved_bytes` を概算し、未観測がある場合は note を追加（`src/engine.rs`）
- ui: 推定値であることが分かるよう `unobserved≈` 表示に変更（`src/ui/mod.rs`）
- docs/tests: 未観測バイトが概算である旨を追記し、権限不足時に `unobserved_bytes > 0` になることを統合テストで担保（`docs/system-data.md`, `tests/cli_safety.rs`）

### 2026-01-02: doctor のノート表示整形

- ui: Summary notes を重要度で並べ替え（System Data → 未観測/ヒント → その他）（`src/ui/mod.rs`）
- tests: System Dataノートが未観測ノートより先に出ることを統合テストで担保（`tests/cli_safety.rs`）

### 2026-01-02: report --markdown の安定化

- markdown: Findings/Actions/IDs の出力順を安定化（同サイズ時はidで決定、Actionsはrisk→size→idで整列）（`src/cli/mod.rs`）
- docs/tests: `docs/report.md` を追加し、アクションの整列順を統合テストで担保（`docs/report.md`, `tests/report_markdown.rs`）

### 2026-01-02: fix の対象指定UX追加（Action ID対応）

- fix: `--target` で Finding ID に加えて Action ID も受理し、未知ターゲット時のヒントを強化（`src/cli/mod.rs`）
- docs/tests: Action ID での絞り込みをREADMEに追記し、統合テストで担保（`README.md`, `tests/cli_safety.rs`）

### 2026-01-02: fix 出力の整形（RUN_CMD/SHOW_INSTRUCTIONS）

- ui: SHOW_INSTRUCTIONS は `--verbose` 時にMarkdown本文の抜粋ブロックを表示し、reportの体裁に寄せた（`src/ui/mod.rs`）

### 2026-01-02: snapshots 未観測時の根拠/導線強化

- snapshots: tmutil/diskutil が失敗した場合に、次の確認手順（Full Disk Access / `sudo ...` / Disk Utility）を Action として付与（`src/rules/mod.rs`）
- docs/tests: 「未観測」時の共通確認手順を `docs/snapshots.md` に追記し、疑似tmutil/diskutilで統合テストを追加（`docs/snapshots.md`, `tests/snapshots_unobserved.rs`）

### 2026-01-02: snapshots status 表示改善（未観測の理由/次アクション）

- ui: 未観測のfindingに `reason`（evidenceのstat要約）と `next`（recommended action）を表示（`src/ui/mod.rs`）
- tests: 疑似tmutil/diskutilで human 出力にも `reason/next` が出ることを統合テストで担保（`tests/snapshots_unobserved.rs`）

### 2026-01-02: snapshots status JSONの見方をdocs化

- docs: `recommended_actions`（Finding → Action）と `related_findings`（Action → Finding）の対応を追えるよう、JSONの見方を追記（`docs/snapshots.md`）
- tests: 疑似tmutil/diskutilのJSONで `related_findings` が埋まっていることも統合テストで担保（`tests/snapshots_unobserved.rs`）

### 2026-01-02: doctor に snapshots サマリ統合

- engine/ui: `doctor` が snapshots 所見も含め、末尾に `Snapshots:` セクションを追加（未観測は reason/next を表示）
- tests: `doctor --json` に snapshots findings が含まれること、`doctor --top 1` に `Snapshots:` が出ることを統合テストで担保（`tests/snapshots_unobserved.rs`）

### 2026-01-02: doctor の実行時間/タイムアウト整備

- doctor: 外部コマンド（`docker`/`tmutil`/`diskutil`）を time budget 内で試行し、時間切れは未観測として継続（`src/engine.rs`, `src/rules/mod.rs`）
- tests: 複数の外部コマンドがハングしても `doctor` が長時間ブロックしないことを統合テストで担保（`tests/doctor_timeout_budget.rs`）

### 2026-01-02: fix --apply のトランザクションログ

- logs: `fix --apply` 実行時に `~/.config/macdiet/logs/` へJSONログを保存（実行日時/対象/結果、TRASH_MOVEは復元可能）（`src/logs/mod.rs`）
- fix: 適用後にログパスを表示し、エラー件数もサマリに含める（`src/cli/mod.rs`, `src/actions/mod.rs`）

### 2026-01-02: snapshots thin/delete の実行ログ

- logs: R3操作（`snapshots thin/delete`）の実行結果（コマンド/exit/stdout/stderr）を `~/.config/macdiet/logs/` に保存（`src/logs/mod.rs`, `src/cli/mod.rs`）

### 2026-01-02: TUI（Claude Code風）UX仕様（案）

- docs: 既存CLIを維持しつつ `macdiet ui` として追加するTUIの画面/キー操作/安全モデル統合/段階的ロードマップを仕様化（`docs/ui.md`）

### 2026-01-02: `macdiet ui`（TUI）Phase 1

- tui: `macdiet ui` を追加し、Home（コマンド選択/簡易検索）→ doctor/snapshots status 実行 → Findings/Actions/Notes の閲覧を実装（`src/tui/mod.rs`, `src/cli/mod.rs`）
- safety: 非TTYでは `ui` を拒否（exit=2）を統合テストで担保（`tests/exit_codes.rs`）

### 2026-01-02: doctor のサイズ推定パフォーマンス改善

- scan: 既知ディレクトリのサイズ推定を time budget 化し、まず `du -sk` を試し、失敗時はwalkdirで時間切れまでベストエフォート（`src/scan/mod.rs`, `src/rules/mod.rs`）

### 2026-01-02: Homebrew `brew cleanup` の失敗診断/修復と sudo 実行の整合

- fix(RUN_CMD): `brew cleanup` の「警告exit=1」を成功扱い（警告あり）として扱い、実失敗時は原因（権限/該当パスなど）を結果画面に要約（`src/actions/mod.rs`, `src/tui/mod.rs`, `src/logs/mod.rs`）
- tui: `Fix your permissions on:` を検出した場合に、結果画面から修復（`chmod`/必要なら`chown`）へ遷移できる導線を追加（R2/R3、キーガイド常時表示、typed confirm 維持）（`src/tui/mod.rs`, `docs/ui.md`）
- sudo互換: `sudo macdiet ui` で起動しても「元ユーザーの home_dir」を維持し、ユーザー環境コマンド（`brew`/`xcrun`/`docker`）は元ユーザー権限で実行して Homebrew の root 実行拒否や “rootのホームを触る” 事故を回避（`src/platform/mod.rs`, `src/engine.rs`, `src/actions/mod.rs`, `src/rules/mod.rs`）


### 2026-01-02: TUI Phase 2（Fix dry-run）

- tui: Fix (dry-run) の候補一覧/リスクフィルタ（1/2/3）/複数選択/詳細表示（related findings 含む）を追加（`src/tui/mod.rs`）
- ux: Help から元の画面へ戻れるように改善（`src/tui/mod.rs`）
- tests: Fix候補のフィルタ/整列/選択trimのユニットテストを追加（`src/tui/mod.rs`）
- docs: UIの実装状況を更新（`docs/ui.md`, `README.md`）

### 2026-01-02: TUI Phase 3（Fix apply: R1のみ）

- tui: Fix 画面から R1/TRASH_MOVE のみを typed confirm（yes→trash）で適用し、結果/ログパスを表示（`src/tui/mod.rs`, `src/cli/mod.rs`）
- safety: `--dry-run` ではTUI applyを無効化（`src/cli/mod.rs`, `src/tui/mod.rs`）
- tests: apply対象のフィルタリング（R1/TRASH_MOVEのみ）をユニットテストで担保（`src/tui/mod.rs`）

### 2026-01-02: TUI Phase 4a（Scan deep: defaults）

- tui: Home から `scan --deep` 相当（scope/excludeはconfig既定、max_depth=3, top_dirs=20）を実行し、Reportビューで閲覧できるように追加（`src/tui/mod.rs`, `src/cli/mod.rs`）
- notes: 推定が未観測/低信頼になり得る旨をREADMEに追記（`README.md`）

### 2026-01-02: TUI Phase 4b（Scan deep: config）

- tui: Scan deep の scope/max_depth/top_dirs/exclude をTUIで編集して実行できる画面を追加（`src/tui/mod.rs`）
- docs: UIの実装状況を更新（`docs/ui.md`, `README.md`）

### 2026-01-02: TUI Phase 5（Logs viewer）

- tui: `~/.config/macdiet/logs/` のログ一覧/詳細（raw JSON）をTUIで閲覧できる画面を追加（`src/tui/mod.rs`）
- docs: UIの実装状況を更新（`docs/ui.md`, `README.md`）

### 2026-01-02: TUI Phase 6（allowlisted R2/RUN_CMD 実行）

- actions/logs: allowlisted RUN_CMD（初期: `xcrun simctl delete unavailable`）を判定し、実行ログ（stdout/stderr/exit）を `~/.config/macdiet/logs/` へ保存（`src/actions/mod.rs`, `src/logs/mod.rs`）
- tui: Fix 画面から allowlisted R2/RUN_CMD を typed confirm（unavailable→run）で実行し、結果/ログパスを表示。Logs viewer でもサマリ抽出を追加（`src/tui/mod.rs`）
- docs/tests: Phase 6 の実装状況を docs に反映し、allowlist/log のユニットテストを追加（`docs/ui.md`, `README.md`, `src/actions/mod.rs`, `src/logs/mod.rs`）

### 2026-01-02: T57 日本語ローカライズ（完全日本語化）

- CLI/TUI: ユーザー向けメッセージ/見出し/ヒント/エラーブロックを日本語化（安全確認トークンは意図的に英字のまま）
- rules: Finding/Action のタイトル・注記（影響/ヒント）を日本語化し、表示の一貫性を改善
- report: `report --markdown` の見出し/ラベル（所見/アクション/根拠/手順/種類）を日本語化
- 表示: 日本語の表示幅に合わせるため `unicode-width` を導入し、表の列崩れを抑制
- tests: 文字列一致の期待値を日本語に更新し、`cargo test` を通過

### 2026-01-02: T58 TUI 入力キー競合の解消 + キーガイド改善

- tui: typed confirm で `b` が「戻る」に奪われないように修正（例: `unavailable` を最後まで入力できる）（`src/tui/mod.rs`）
- tui: ホームの検索入力中は `q` を終了ではなく入力として扱うように修正（`src/tui/mod.rs`）
- ux: フッターを2行のキーガイドにして、画面ごとの主要操作を常時表示（`src/tui/mod.rs`）
- tests: 入力キー競合（`b`/`q`）の回帰ユニットテストを追加（`src/tui/mod.rs`）

### 2026-01-02: T59 CoreSimulator の RUN_CMD 提案精度改善

- rules: `xcrun simctl list devices unavailable` の結果に応じて、`xcrun simctl delete unavailable` を「unavailable がある時だけ」提案（`src/rules/mod.rs`）
- tests: R2プレビューで `xcrun` をスタブし、unavailable 有/無の両ケースを統合テストで担保（`tests/fix_r2_preview.rs`）

### 2026-01-02: T55 CLI fix apply Phase 2（allowlisted RUN_CMD 実行）

- cli: `macdiet fix --apply --risk R2` で allowlisted R2/RUN_CMD を typed confirm（`unavailable`→`run`）付きで実行し、ログ保存＆外部コマンド失敗は exit=20（`src/cli/mod.rs`）
- ui/docs: R2+ の扱い（既定はプレビュー、CLIで実行できるのは R1/TRASH_MOVE と allowlisted R2/RUN_CMD のみ）を出力/READMEに反映（`src/ui/mod.rs`, `README.md`）
- tests: R2プレビューの期待文言を更新（`tests/fix_r2_preview.rs`）

### 2026-01-02: T56 TUI Phase 7（横断検索/フィルタ）

- tui: `/` でフィルタ入力モード（Backspace/Ctrl-U/Enter/Esc）を追加し、入力中は `q/b/r` 等の操作キーに奪われないようにした（`src/tui/mod.rs`）
- tui: ReportView（所見/アクション/注記）・Fix候補・Logs一覧をフィルタ文字列で絞り込み、表示件数/総件数を明示（`src/tui/mod.rs`）
- logs: Logs一覧の検索性を上げるため、ファイル名/種別/JSON内の主要キー/コマンド/パスから `search_text` を生成してフィルタ対象に含めた（`src/tui/mod.rs`）
- tests: フィルタの基本挙動（Report/Logs）をユニットテストで担保し、`cargo test` を通過（`src/tui/mod.rs`）

### 2026-01-02: T60 fix apply UX（実行対象の明確化）

- cli: `fix --apply` 時に「実行対象（R1/TRASH_MOVE / allowlisted R2/RUN_CMD）」「対象外（プレビューのみ）」を明示し、対象外は理由付きでサンプル表示（`src/cli/mod.rs`）
- cli: 許可リスト外の RUN_CMD が候補に含まれていても apply 全体を失敗させず、対象外（プレビューのみ）として扱う（`src/cli/mod.rs`）
- tests/docs: 分類ロジックのユニットテストを追加し、READMEに補足を追記（`src/cli/mod.rs`, `README.md`）

### 2026-01-02: T61 TUI UX（Vim風 j/k ナビゲーション）

- tui: Home/Report/Fix/Logs/ScanConfig など主要画面で `j/k` を `↑↓` と同等に扱い、リスト移動/スクロールを改善（`src/tui/mod.rs`）
- tui: フッターキーガイドとヘルプ表示を `↑↓/j/k` に追従（`src/tui/mod.rs`）
- tests: Home での `j/k` の挙動（入力モード/非入力モード）をユニットテストで担保（`src/tui/mod.rs`）

### 2026-01-02: T62 TUI エラー復帰 + 適用対象の誤選択ガイド

- tui: Error画面の `b/Esc` で元の画面へ戻れるようにし、Fix画面→Error→Fix画面の往復で再dry-runが不要になった（`src/tui/mod.rs`）
- tui: SHOW_INSTRUCTIONS 等の「適用対象外」だけを選んだ状態で `p` を押したとき、理由と正しい候補（例: TRASH_MOVE）を提示するように改善（`src/tui/mod.rs`）
- tests: 上記の回帰ユニットテストを追加し、`cargo test` を通過（`src/tui/mod.rs`）

### 2026-01-02: T63 allowlisted RUN_CMD（brew cleanup）をR1として追加

- rules: `homebrew-cache-cleanup` を SHOW_INSTRUCTIONS から RUN_CMD（`brew cleanup`）へ変更し、候補の推定サイズを引き継ぐようにした（`src/rules/mod.rs`）
- actions: allowlist に `brew cleanup`（R1）を追加し、typed confirm（`cleanup`→`run`）でのみ実行できるようにした（`src/actions/mod.rs`）
- tui/cli: allowlisted RUN_CMD を R1/R2 として扱い、TUIは `p`（RUN_CMDのみ選択時は実行確認へ遷移）/`x` の両方で導線を提供（`src/tui/mod.rs`, `src/cli/mod.rs`）
- docs/tests: README/UI仕様と統合テストの期待文言を追従し、`cargo test` を通過（`README.md`, `docs/ui.md`, `tests/fix_r2_preview.rs`）

### 2026-01-02: T64 `brew cleanup` の exit_code=1 を警告として扱う

- actions: allowlisted RUN_CMD の実行結果を評価する関数を追加し、`brew cleanup` は「明確なエラーが無い exit_code=1」を警告（成功扱い）に分類（`src/actions/mod.rs`）
- tui/cli/logs: UI表示は「OK（警告あり）」、CLIはexit=20にせず継続、ログは `status=ok_with_warnings` を記録（`src/tui/mod.rs`, `src/cli/mod.rs`, `src/logs/mod.rs`）
- tests: 評価ロジック/ログの `ok_with_warnings` をユニットテストで担保し、`cargo test` を通過（`src/actions/mod.rs`, `src/logs/mod.rs`）

### 2026-01-02: T65 `brew cleanup` の実失敗（権限不足など）を要約表示

- actions: `brew cleanup` が「warningsではない失敗」だった場合に、Error行・権限修正対象パス・次アクション（`brew doctor`）を要約して返すように改善（`src/actions/mod.rs`）
- tests: 上記の要約が含まれることをユニットテストで担保し、`cargo test` を通過（`src/actions/mod.rs`）

### 2026-01-02: T66 `brew cleanup` の権限エラーをTUI内で修復できる導線

- actions: `Fix your permissions on:` を検出した場合に、allowlisted RUN_CMD として「権限修復（`chmod -R u+rwX`）」アクションを提案できるようにした（`src/actions/mod.rs`）
- tui: RUN_CMD 結果画面で修復提案があると `f` キーで修復アクションの typed confirm に遷移できるようにし、キーガイド/ヘルプにも追記（`src/tui/mod.rs`, `docs/ui.md`）
- tests: allowlist/提案ロジック/TUIキー導線の回帰テストを追加し、`cargo test` を通過（`src/actions/mod.rs`, `src/tui/mod.rs`）

### 2026-01-03: T69〜T71/T70 TUI実行導線の拡張

- tui: TRASH_MOVE/RUN_CMD 実行後に Fix(dry-run) を自動更新し、`r` を押さなくても「完了した項目が残る」問題を解消（`src/tui/mod.rs`）
- tui: Home に「ユーティリティ」画面を追加し、doctor の所見に依存せず allowlisted RUN_CMD を実行できる導線を追加（`src/tui/mod.rs`）
- rules/actions: `npm`/`yarn`/`pnpm` のキャッシュ整理を R1/RUN_CMD として提案し、許可リスト化（typed confirm + logs）（`src/rules/mod.rs`, `src/actions/mod.rs`, `src/tui/mod.rs`）
- 次の優先: R2（例: Docker prune 等）の allowlisted RUN_CMD 化＋TUI実行（T72）

### 2026-01-03: T72 R2（Docker prune 等）のTUI実行

- actions: `docker builder prune` / `docker system prune` を allowlisted RUN_CMD（typed confirm: `builder-prune`/`system-prune`→`run`）として追加（`src/actions/mod.rs`）
- tui: ユーティリティ画面で R2 の Docker prune を選択→typed confirm→実行できるように拡充（`src/tui/mod.rs`）
- tui: RUN_CMD 結果画面の `f/g` 修復から実行した場合も、戻り先（Utilities/Fix）を引き継ぐように修正（`src/tui/mod.rs`）
- docs/tests: UI仕様の例を更新し、`cargo test` を通過（`docs/ui.md`, `src/actions/mod.rs`）

### 2026-01-03: T75/T76 R2の「確認→実行/削除」をTUI内で完結

- tui: Fix画面の `c` から「個別削除（ゴミ箱へ移動）」へ遷移し、Archives/DeviceSupport/CoreSimulator(unavailable) の候補を列挙→複数選択→typed confirm（yes→trash）→TRASH_MOVE を実行（`src/tui/mod.rs`）
- actions: TRASH_MOVE の許可リストを「ベース配下の子孫パス」に拡張し、個別削除の安全域を確保（`src/actions/mod.rs`）
- rules/actions: `docker-storage-df` を RUN_CMD に変更し allowlist 化、TUI（Fix/Utilities）から `docker system df` を実行→結果/ログを確認可能に（`src/rules/mod.rs`, `src/actions/mod.rs`, `src/tui/mod.rs`）
- safety: 最大リスクゲート（R2）・typed confirm・ログ（`~/.config/macdiet/logs`）を維持し、戻る操作で自動更新される導線を追加（`src/tui/mod.rs`）
- tests: allowlist/パース/候補生成のユニットテストを追加し、`cargo test` を通過
