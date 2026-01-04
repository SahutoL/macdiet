# macdiet: now-task

最終更新: 2026-01-04

## 進行中（In Progress）

（空）

## 進行予定（Planned）

（空）

## 完了済み（Done）

- [x] T73: TUIから snapshots thin/delete を実行（DoD: R3 の二段階確認をTUIに実装し、結果/ログ閲覧まで一連で完結。既存CLIの安全モデル（非TTY拒否/exit=20/ログ）を維持。`cargo test`）→ `src/tui/mod.rs`, `README.md`, `docs/ui.md`（`cargo test`）
- [x] T78: コミット前のリポジトリ衛生チェック（DoD: 個人情報/端末依存/生成物/モック文言が混入していないことを確認し、必要な修正（例: `.gitignore`/テストデータの一般化）を適用。`cargo test`）→ `.gitignore`, `src/actions/mod.rs`, `src/tui/mod.rs`（`cargo test`）
- [x] T77: Fix画面の操作ガイド（p/x/c）を初心者向けに改善（DoD: 上部ヒントとヘルプに `c` を含め、詳細ペインに「このアクションで使うキー」を表示する。可能なら Enter を「おすすめ操作」にして迷わず実行できる導線を追加。`cargo test`）→ `src/tui/mod.rs`, `docs/ui.md`（`cargo test`）
- [x] T76: `docker system df` をTUI内で実行（DoD: `docker-storage-df` を allowlisted RUN_CMD として実行でき、結果/ログがTUIで確認できる。`cargo test`）→ `src/rules/mod.rs`, `src/actions/mod.rs`, `src/tui/mod.rs`（`cargo test`）
- [x] T75: R2 個別削除（TUI内で選択→ゴミ箱へ移動）（DoD: `xcode-archives-review`/`xcode-device-support-review`/`coresimulator-devices-xcrun` から「個別削除」画面へ遷移し、候補の複数選択→typed confirm→Trash移動ができる。許可リスト（ベースパス限定）と最大リスク=R2ゲートを維持し、実行後は一覧が自動更新される。ログが残る。テスト追加、`cargo test`）→ `src/tui/mod.rs`, `src/actions/mod.rs`（`cargo test`）
- [x] T74: ユーティリティ起点の RUN_CMD 修復後に正しい画面へ戻る（DoD: Utilities から RUN_CMD→失敗→`f/g` 修復→結果で `b` を押すと Utilities に戻り、Fix(dry-run) が走らない。テスト追加、`cargo test`）→ `src/tui/mod.rs`（`cargo test`）
- [x] T72: R2 の手順もTUIで実行可能化（DoD: R2/SHOW_INSTRUCTIONS を棚卸しし、必要なら事前チェック付きで RUN_CMD（許可リスト）化。最大リスク=R2 のゲート、typed confirm、ログ、失敗時の次アクション提示を整備。`cargo test`）→ `src/actions/mod.rs`, `src/tui/mod.rs`, `docs/ui.md`（`cargo test`）
- [x] T70: R1（ゴミ箱移動以外）の実行導線を拡張（DoD: 既存の R1/SHOW_INSTRUCTIONS を棚卸しし、「TUI内で実行できるべき安全な手順」を RUN_CMD（許可リスト）として提供。Fix画面で選択→`x`（typed confirm）で実行でき、結果/ログが残る。`cargo test`）→ `src/rules/mod.rs`, `src/actions/mod.rs`, `src/tui/mod.rs`（`cargo test`）
- [x] T71: TUIに「ユーティリティ（許可リストRUN_CMD）」を追加（DoD: doctor の所見に依存せず、代表的な R1 RUN_CMD（例: `brew cleanup`）をTUIから実行できる画面/導線を追加。安全モデル（最大リスク/typed confirm/ログ/非TTY拒否）を維持。`cargo test`）→ `src/tui/mod.rs`（`cargo test`）
- [x] T69: TUIの実行後に Fix を自動整合（DoD: TRASH_MOVE/RUN_CMD 実行後に Fix(dry-run) へ戻るとき自動で再計算（または実行済みを非表示）され、`r` を押さなくても「完了した項目が残る」問題が再現しない。選択状態も破綻しない。テスト追加、`cargo test`）→ `src/tui/mod.rs`（`cargo test`）
- [x] T68: `sudo macdiet ui` でも user環境を維持（DoD: `SUDO_UID/SUDO_GID` を検出したら home_dir を元ユーザーに解決し、`brew`/`xcrun`/`docker` 等の user-context コマンドは元ユーザー権限で実行して Homebrew の root 拒否や user home のズレを防ぐ。テスト/ドキュメント追従、`cargo test`）→ `src/platform/mod.rs`, `src/engine.rs`, `src/cli/mod.rs`, `src/actions/mod.rs`, `src/rules/mod.rs`, `src/tui/mod.rs`, `Cargo.toml`, `docs/ui.md`（`cargo test`）
- [x] T67: `brew cleanup` 権限エラーの追加修復（DoD: `chmod` でも解決しないケース向けに、TUI結果で「所有者修復（chown、R3/要sudo）」等の次善策を提示し、キー操作で実行/または手順表示できる。最大リスクのゲートも尊重し、テスト/ドキュメント追従、`cargo test`）→ `src/actions/mod.rs`, `src/tui/mod.rs`, `docs/ui.md`（`cargo test`）
- [x] T66: `brew cleanup` の権限エラーをTUI内で修復できる導線を追加（DoD: `Fix your permissions on:` を検出したら、TUI結果で「権限修復（chmod）」を提案し、キー操作で実行→再試行できる。ログ/テスト/ドキュメント追従）→ `src/actions/mod.rs`, `src/tui/mod.rs`, `docs/ui.md`（`cargo test`）
- [x] T65: `brew cleanup` 失敗時（権限問題など）の原因をTUI結果に要約表示し、次の対処（`brew doctor` 等）を案内（DoD: `homebrew-cache-cleanup` が失敗したとき「なぜ/どこ/次に何を」が結果画面で分かる。テスト追加、`cargo test`）→ `src/actions/mod.rs`（`cargo test`）
- [x] T64: `brew cleanup` の exit_code=1 を「警告（成功扱い）」として扱い、TUI/CLI/ログの表示を改善（DoD: exit_code=1 かつ明確なエラーが無い場合に OK（警告あり）として表示され、ログは `ok_with_warnings` になる。`cargo test`）→ `src/actions/mod.rs`, `src/tui/mod.rs`, `src/cli/mod.rs`, `src/logs/mod.rs`（`cargo test`）
- [x] T63: allowlisted RUN_CMD（brew cleanup）をR1としてTUI/CLIから実行（DoD: `homebrew-cache-cleanup` を許可リストRUN_CMDに変更し、typed confirm + logs 付きで実行できる。TUIは `p/x` どちらでも導線が分かる。テスト/ドキュメント追従）→ `src/rules/mod.rs`, `src/actions/mod.rs`, `src/tui/mod.rs`, `src/cli/mod.rs`, `README.md`, `docs/ui.md`, `tests/fix_r2_preview.rs`（`cargo test`）
- [x] T62: TUI エラー復帰 + 適用対象の誤選択ガイド（DoD: Error画面の b/Esc で元画面へ戻れ、SHOW_INSTRUCTIONS 選択時の `p` は分かりやすい案内を出す）
- [x] T61: TUI UX（DoD: Vim風の `j/k` で主要リストの移動ができ、ヘルプ/キーガイドに反映）
- [x] T60: fix apply UX（DoD: apply時に実行対象（R1/TRASH_MOVE と allowlisted R2/RUN_CMD）をより明確に表示し、未実行アクションの扱いを統一）
- [x] T56: TUI Phase 7（DoD: Fix/Scan/Snapshots/Logs を横断する検索・フィルタを追加）
- [x] T55: CLI fix apply Phase 2（`macdiet fix --apply --risk R2` で allowlisted RUN_CMD を実行（typed confirm + logs + exit=20）。ドキュメント/テスト追従、`cargo test`）
- [x] T59: CoreSimulator の RUN_CMD 提案精度改善（`xcrun simctl list devices unavailable` で必要性を確認し、必要な時だけ `xcrun simctl delete unavailable` を提案。テスト追加・`cargo test`）
- [x] T58: TUI 入力キー競合の解消 + キーガイド改善（`src/tui/mod.rs`、typed confirm で `b` が入力できるよう修正、ホーム検索で `q` が入力できるよう修正、フッターを2行キーガイド化、ユニットテスト追加、`cargo test`）
- [x] T57: 日本語ローカライズ（DoD: CLI/TUIのユーザー向けメッセージとヘルプを日本語化し、テスト/ドキュメントを追従）
- [x] T54: TUI Phase 6（DoD: R2のうちSimulator整理など“安全寄り”な RUN_CMD を allowlist + typed confirm でTUIから実行できる）
- [x] T53: TUI Phase 5（DoD: `~/.config/macdiet/logs/` のログをTUIで閲覧できる）
- [x] T52: TUI Phase 4b（DoD: Scan deep のパラメータ（scope/max_depth/top_dirs/exclude）をTUIで編集できる）
- [x] T51: TUI Phase 4a（DoD: Scan deep（デフォルト設定）をTUIから実行でき、結果（Top dirs）を閲覧できる）
- [x] T50: TUI Phase 3（DoD: Fix apply（R1のみ）をTUIから実行でき、typed confirm + ログ表示を維持）
- [x] T49: TUI Phase 2（DoD: Fix dry-run をTUIで閲覧/選択でき、R1 applyはまだ実行しない）
- [x] T46: doctorのサイズ推定パフォーマンス改善（DoD: 巨大ディレクトリでもdoctorが目標時間に収まりやすい推定戦略を追加）
- [x] T48: `macdiet ui`（TUI）Phase 1（DoD: TTY上でHome→doctor実行→Findings/Actions/Notes閲覧ができ、非TTYではexit=2で拒否）
- [x] T47: TUI（Claude Code風）UX仕様（DoD: `docs/ui.md` に画面/キー操作/安全モデル統合/段階的ロードマップを明記）
- [x] T45: snapshots thin/delete の実行ログ（DoD: R3操作の実行結果（コマンド/exit/出力）をローカルに記録する）
- [x] T44: `fix --apply` のトランザクションログ（DoD: どのActionをいつ/何に対して実行したかをローカルに記録する）
- [x] T43: `doctor` の実行時間/タイムアウト整備（DoD: snapshots統合後も30秒目標を守るため、外部コマンドの扱いを整理）
- [x] T42: `doctor` に snapshotsサマリ統合（DoD: doctorにsnapshot所見を含め、未観測はunobservedとして扱う）
- [x] T41: `snapshots status` のJSON整形（DoD: action/findingの関連が追えるように整形方針をdocs化）
- [x] T40: `snapshots status` の表示改善（DoD: 未観測時の理由/次アクションを短く整形）
- [x] T39: `doctor` のスナップショット根拠強化（DoD: diskutil/tmutilの未観測時に次の確認手順を具体化）
- [x] T38: `fix` の出力整形（DoD: RUN_CMD/SHOW_INSTRUCTIONS の表示を report/doctor と揃える）
- [x] T37: `fix` の対象指定UX追加（DoD: `--target` に Action ID も受理し、ヒントを強化）
- [x] T36: `report --markdown` の安定化（DoD: 表示順/改行の揺れを最小化し、サンプル出力をdocsに追加）
- [x] T35: `doctor` のノート表示整形（DoD: System Data/未観測の重要ノートを上位に固定表示）
- [x] T34: `doctor` の未観測バイト推定改善（DoD: 権限不足時の「未観測」推定/表示を整理し、docsに方針を明記）
- [x] T33: `doctor` のSystem Data解説強化（DoD: Apple定義を短く説明、未観測/権限の導線を整理）
- [x] T32: `fix` R2提案の追加（DoD: docker/simulator等のR2 actionを提案し、既定は実行不可）
- [x] T31: `fix` のR2プレビュー強化（DoD: R2 actionの要約/影響の統一、実行は次版）
- [x] T30: `report --markdown` の整形強化（DoD: Action/impact/pathsを読みやすく、evidence任意）
- [x] T00: エージェント運用の体系化（`AGENTS.md`/`rules.md` 作成、`now-task.md`/`review.md` 雛形）
- [x] T01: プロジェクト雛形（Rust）初期化（`cargo init`、モジュール雛形、`cargo test`）
- [x] T02: CLI骨格（`doctor/scan/snapshots/fix/report` + 共通オプション）追加（未実装コマンドはプレースホルダ）
- [x] T03: `core`（Finding/Action/Report）とJSONスキーマ（golden test）追加（`tests/report_json_golden.rs`）
- [x] T11: `scan --deep` 初期実装（scope preset/path、`--max-depth`/`--top-dirs`/`--exclude`、JSON出力）
- [x] T04: 既知パスの浅い検出ルール（Xcode/Simulator/Docker/主要キャッシュ）実装（doctorの上位原因検出、根拠パス提示）
- [x] T06: `snapshots status`（tmutil/diskutil）実装（可能範囲で検出、Disk Utility導線、失敗時は未観測として表示）
- [x] T08: `fix` dry-run（R1最大、ホワイトリスト検証、TRASH_MOVE提案、`--apply`無しで変更ゼロ）
- [x] T12: `scan --deep` UX改善（TTY時の進捗表示を追加、`--json`/非TTYでは抑制）
- [x] T13: `fix --apply`（R1/TRASH_MOVE限定）実装（TTY+確認、ホワイトリスト検証、`~/.Trash` 移動）
- [x] T05: `doctor` UX改善（表/色/進捗/未観測/エラーブロック）
- [x] T07: `report --json/--markdown` 改善（evidence既定非表示、`--include-evidence`で表示、`--json`のglobal化）
- [x] T09: 統合/セーフティテスト追加（`tests/cli_safety.rs`、dry-run no-op、non-TTY apply拒否、evidence既定非表示）
- [x] T10: OSSドキュメント追加（`README.md` / `docs/` / `SECURITY.md`）
- [x] T14: `config` 実装（TOML読込、`config --show`、report/fix/scanへ反映、統合テスト追加）
- [x] T15: `completion` 実装（bash/zsh/fish、`tests/cli_safety.rs` に簡易テスト追加）
- [x] T16: Config優先順位（CLI > env > config > default）実装（`MACDIET_*` で主要キー上書き、`tests/env_precedence.rs` 追加）
- [x] T17: `fix --interactive`（TTYでAction候補を番号選択、選択結果をdry-run/`--apply`に反映、選択パーサのユニットテスト追加）
- [x] T18: 終了コードの整備（invalid args=2/致命=10/外部コマンド=20、`tests/exit_codes.rs` 追加）
- [x] T19: `snapshots thin` 実装（R3/二段階確認、`--dry-run` 対応、失敗はexit=20で扱う）
- [x] T20: ドキュメント更新（README/docs/SECURITYを現状に追従、env/exit code等を追記）
- [x] T21: `snapshots delete` の設計/実装（検出したUUIDのみ受理、TTY+二段階確認、外部コマンド失敗はexit=20）
- [x] T22: `snapshots` ID検出の改善（Name/UUIDの抽出・UUIDへの一意解決、`--id <uuid|name>` に対応、docs/テスト追加）
- [x] T23: `fix` UX改善（Impact note表示、SHOW_INSTRUCTIONSの要約表示、統合テスト追加）
- [x] T24: exit=20の方針整理（外部コマンド実行系はexit=20で統一、doctor/scanは継続可能なら警告に落とす）
- [x] T25: `doctor` の検出カテゴリ拡充（DocSets/Device Logs/Gradle cachesを追加、TRASH_MOVEホワイトリスト拡張、統合テスト追加）
- [x] T26: `doctor` のDocker系検出の強化（`docker system df` を併用して根拠に追加、失敗時はファイル推定にフォールバック）
- [x] T27: `scan --deep` のscope拡充（devスコープに`.gradle`を追加、README更新、統合テスト追加）
- [x] T28: `doctor` 表示の一貫性改善（表示件数の明示、推奨Actionのフィルタ/表示件数を整備、統合テスト追加）
- [x] T29: `fix` の対象指定UX改善（Actionのtargets表示、未知`--target`の検出/ヒント、README更新、統合テスト追加）
