# TUI（Claude Code風）UX仕様（案）

macdiet は現在「サブコマンド型CLI（`doctor/scan/snapshots/fix/report`）」として成立している。これを維持しつつ、TTY上で動く **フルスクリーンTUI（疑似GUI）** を追加して、Claude Code CLI のような「コマンド選択→結果のパネル表示→安全な提案→（限定的に）実行」を一連の体験として提供する。

この文書は **仕様（設計）**。安全モデル（R0既定、ホワイトリスト、typed confirm、stdout/stderr分離）を弱めないことを最優先とする。

実装状況:

- Phase 1（最小）は `macdiet ui` として実装済み（読み取り専用: doctor/snapshots status の実行・閲覧）
- Phase 2 は実装済み（Fix dry-run の候補閲覧/リスクフィルタ/複数選択/詳細表示）
- Phase 3 は実装済み（Fix apply: R1/TRASH_MOVEのみ、typed confirm（yes→trash）、実行後ログパス表示）
- Phase 4a は実装済み（Scan deep: デフォルト設定で実行し、Top dirs を閲覧）
- Phase 4b は実装済み（Scan deep: scope/max_depth/top_dirs/exclude をTUIで編集して実行）
- Phase 5 は実装済み（Logs: `~/.config/macdiet/logs/` のログ一覧/詳細閲覧）
- Phase 6 は実装済み（Fix: allowlisted RUN_CMD（例: `brew cleanup`, `xcrun simctl delete unavailable`）を typed confirm で実行し、ログへ記録）
- Phase 7 は実装済み（`/` で横断フィルタ: ReportView（所見/アクション/注記）/ Fix候補 / Logs一覧を絞り込み）
- Phase 8 は実装済み（Utilities: doctor の所見に依存せず、allowlisted RUN_CMD を選択→typed confirm で実行）
- Phase 9 は実装済み（個別削除: Fix画面の一部R2候補から「個別削除（ゴミ箱へ移動）」へ遷移し、候補の複数選択→typed confirm→TRASH_MOVE を実行）

## 1. 目的

- 端末上で「GUI的」に操作できる導線を作る（コマンドパレット、パネル表示、ショートカット）。
- `doctor` の結果（Findings/Actions/Notes）を **探索・理解** しやすくする（フィルタ、詳細、根拠の確認）。
- `fix` / `snapshots` の実行を **より安全** にする（実行前の影響確認、typed confirm、ログ表示）。
- 既存の自動化用途を壊さない（従来CLIと `--json` は維持し、TUIは別入口にする）。

## 2. 非目的（Non-Goals）

- `macdiet doctor --json` 等の既存CLIを置き換えない（破壊的変更を避ける）。
- 端末外のネイティブGUIを作らない（SwiftUI等は対象外）。
- R2+ の自動実行を既定で解禁しない（v0.1の安全方針を維持）。
- `sudo` 昇格をツール側で自動化しない（ユーザーが明示的に `sudo macdiet ...`）。

## 3. 入口（CLI仕様案）

- 新コマンド: `macdiet ui`（別名候補: `macdiet tui`）
- 互換性: 既存の `doctor/scan/...` はそのまま

推奨のガード:

- `macdiet ui` は **TTY必須**（stdin+stdout）
- `macdiet ui` は `--json` と併用不可（stdoutがUIになるため）
- `--timeout` 等のグローバル設定は `ui` 内で反映（外部コマンド・doctor time budget）

## 4. 情報設計（画面構成）

共通レイアウト（案）:

- **上部バー**: `macdiet` / 現在の画面 / `--timeout` / 権限ヒント（unobserved時）
- **左ペイン**: コマンド/セクション（Doctor / Fix / Snapshots / Scan / Report / Config / Logs）
- **中央ペイン**: 一覧（Findings/Actions/Top dirs/Logs）
- **右ペイン**: 詳細（選択中のFinding/Actionの説明、evidence、next steps）
- **下部バー**: 主要キー（`? help` `/: search` `Tab: switch` `q: quit` など）

## 5. 画面とフロー

### 5.1 Home / Command Palette

- 目的: 「何をしたいか」を即座に選べる
- 操作:
  - `:` コマンドパレット（fuzzy検索）
  - `Enter` 実行 / `Esc` 閉じる
- 候補例:
  - `Doctor (quick)` / `Scan (deep)` / `Snapshots status` / `Fix (dry-run)` / `Report export`

### 5.2 Doctor（結果ビューア）

- 実行: `r`（再実行）または Home から開始
- 中央: Findings（サイズ順、risk色）
- 右: Finding詳細（reason/next、notes、evidenceは既定折りたたみ）
- Actionsタブ:
  - 推奨Actionをrisk順に表示（R1→R2→R3）
  - R2+は「手動実行（提案のみ）」を明確に表示

### 5.3 Snapshots（status / thin / delete）

- status:
  - 未観測（tmutil/diskutil失敗/timeout）は reason + next を強調
- thin/delete（R3）:
  - 実行前に「警告パネル」→「typed confirm」へ進む
  - 実行後に **ログ保存先（`~/.config/macdiet/logs/`）** を表示し、必要なら Logs 画面へジャンプ

### 5.4 Fix（dry-run / apply）

- dry-run:
  - 候補Actionを一覧表示（ID/targets/impact）
  - `Space` で複数選択 → 右ペインでまとめて影響確認
- apply（R1のみ）:
  - 実行可能なのは TRASH_MOVE のみ（現行仕様）
  - 実行前に「見込み削減/対象/影響」→「typed confirm（2段階）」を必須化
  - 実行後に **トランザクションログ** を表示し、復元手順（Trash）も提示
- RUN_CMD（allowlisted のみ）:
  - 実行可能なのは allowlist に一致する RUN_CMD のみ（例: `brew cleanup`, `npm cache clean --force`, `docker system prune`, `xcrun simctl delete unavailable`）
  - 実行前に typed confirm（操作トークン→run）を必須化
  - 実行後に **実行ログ** を表示し、Logs 画面で stdout/stderr を確認できる
  - `sudo macdiet ui` で起動している場合でも、ユーザー環境に属する RUN_CMD（例: `brew`, `xcrun`, `docker`）は元ユーザー権限で実行する（Homebrew の root 実行拒否や、root のホームに対する誤操作を防ぐ）
  - 失敗が権限問題（`Fix your permissions on:`）に見える場合、TUIの結果画面から「権限修復（chmod）」や、必要なら「所有者修復（chown、R3/要sudo）」を提案して実行できる

- 個別削除（R2/TRASH_MOVE、限定）:
  - Fix画面の一部R2候補（例: `xcode-archives-review` / `xcode-device-support-review` / `coresimulator-devices-xcrun`）から `c` で遷移
  - 候補を一覧化し、`Space` で複数選択 → `p` で typed confirm（yes→trash） → ゴミ箱へ移動（TRASH_MOVE）
  - パス許可は「ベース配下の子孫パス」に厳格限定し、最大リスク=R2 のゲートを維持

### 5.5 Scan（deep）

- パラメータ入力UI（scope/max_depth/top_dirst/exclude）をTUI上で編集
- 実行中は進捗を表示し、キャンセル導線（`Ctrl-C` / `Esc`）を用意
- 結果は「Top dirs」の一覧＋詳細（パス、推定、error_count）

### 5.6 Report（export）

- `report --json/--markdown` 相当の出力を **ファイルへ書き出し**（TUI内はstdoutがUI）
- 既定は `~/Downloads/` などユーザー領域（明示選択）

### 5.7 Logs（監査ログビューア）

- `~/.config/macdiet/logs/` のログ一覧
- 選択すると詳細（コマンド、exit_code、stdout/stderr、fixのmoved/skipped/errors）

## 6. キーバインド（案）

グローバル:

- `q`: 終了（確認が必要な状態なら警告）
- `?`: ヘルプ（キー一覧/安全モデル）
- `Tab`/`Shift-Tab`: ペイン切替
- `/`: 検索（フィルタ）
- `r`: 再実行（現在画面の主コマンド）

リスト操作:

- `j/k` または `↑/↓`: 移動
- `Enter`: おすすめ操作（現在行。Fixでは TRASH_MOVE/RUN_CMD/個別削除 を自動選択）
- `Space`: 選択（Fix候補など、まとめて実行する場合）
- `p`: 適用（選択中の TRASH_MOVE のみ）
- `x`: 実行（選択中の許可リスト RUN_CMD のみ、原則 1 つずつ）
- `c`: 個別削除（対応するR2候補のみ）

危険操作（typed confirm）:

- Enter連打で進めない（必ず文字入力）
- R1: `yes` → `trash`
- RUN_CMD（allowlisted）: 例 `cleanup` → `run` / `unavailable` → `run`
- RUN_CMD（allowlisted）: 例 `builder-prune` → `run` / `system-prune` → `run`
- R3: `thin`/`delete` → `yes`/UUID 等（現行CLIと整合）

## 7. 安全モデルの統合（必須）

- 既定は **R0（閲覧）**。TUIは「診断→提案」を主とする。
- `fix` の適用（TRASH_MOVE）は **R1のみ**。
- 一部のR2領域（Archives/DeviceSupport/CoreSimulator 等）は、TUIの「個別削除」から **TRASH_MOVE（ゴミ箱移動）** を限定的に実行できる（最大リスクゲート + typed confirm + ログ）。
- RUN_CMD の実行は **allowlisted のみ**（typed confirm 必須）。それ以外の R2+ は基本 **提案のみ**（手順の表示、コピーしやすくする）。
- `snapshots thin/delete` は **R3** として扱い、強い同意・ログ・失敗時exit=20の方針を維持する。
- 権限不足は「0扱い」にせず **未観測** として可視化し、Full Disk Access導線を表示する。

## 8. 実装方針（アーキテクチャ案）

※ここは“実装時に守るべき設計方針”であり、現時点では実装しない。

- `src/tui/` を新設し、TUI専用の入出力層として分離する
- 依存候補: `ratatui` + `crossterm`（フルスクリーン/イベント/描画）
- 既存の `core::Report` / `core::ActionPlan` をそのまま表示に使う（ドメインは共有）
- 進捗は `engine` から **イベント（コールバック/チャネル）** として受け取り、TUIに描画する（indicatif直書きのままだと相性が悪い）
- 端末のraw modeは必ず復旧する（panic/err時のガードが必須）

## 9. 段階的ロードマップ（推奨）

- Phase 1（最小）: `macdiet ui` + Command Palette + Doctor結果ビューア（閲覧/検索/詳細/Export）
- Phase 2: Snapshots status ビュー（未観測 reason/next を強調）＋ Fix dry-run ビュー（候補選択）
- Phase 3: Fix apply（R1のみ）をTUIから実行（typed confirm + log表示）
- Phase 4: Scan deep のパラメータ編集＋進捗表示
- Phase 5: Logs ビューア（修復/監査の一体化）
- Phase 6: Fix の allowlisted RUN_CMD をTUIから実行（typed confirm + log表示）

## 10. オープン課題（決めるべきこと）

- `macdiet`（引数なし）でTUIを起動するか？（推奨: 互換性のため **起動しない**、`macdiet ui` 明示）
- Exportの既定保存先（`~/Downloads` か、カレントか）
- TUI内での `--verbose` 相当（ログペインに集約するか、ステータスバーに流すか）
