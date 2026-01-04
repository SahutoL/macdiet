# macdiet

macOSのストレージ肥大化、とくに「System Data（システムデータ）」として一括計上されがちな領域を **原因カテゴリに分解して可視化 → 安全な改善提案 →（限定的に）実行** する開発者向けCLIです。

> 注意: macdiet は Apple 非公式です。SIP無効化やOS領域改変などの危険な最適化は行いません。

## できること（v0.1）

- `doctor`: 開発者環境で頻出の肥大化要因（Xcode/Simulator/Docker/主要キャッシュ等）を根拠（Evidence）付きで推定し、上位を表示
- `ui`: Claude Code風の対話UIで `doctor`/`scan --deep`/`snapshots status`/`fix`/`logs` を実行・閲覧（Phase 7: R1/TRASH_MOVE の適用 + allowlisted RUN_CMD の実行 + logs閲覧 + 横断フィルタ（`/`）。typed confirm必須）
- `scan --deep`: 指定スコープを集計し、巨大ディレクトリのランキング（深さ制限付き）
- `snapshots status`: Time Machine ローカルスナップショット / APFSスナップショットの状態を可能な範囲で表示（失敗時は未観測として可視化）
- `snapshots thin`: ローカルスナップショットの thin（R3、TTY+二段階確認、`--dry-run` でプレビュー）
- `snapshots delete`: APFSスナップショットの削除（R3、検出してUUIDへ一意に解決できるIDのみ、TTY+二段階確認、`--dry-run` でプレビュー）
- `fix`: R1/TRASH_MOVE は `--apply` で `~/.Trash` へ移動まで実行（TTY+明示確認必須）。RUN_CMD は allowlisted のみ `--apply` で限定実行（入力による確認）。それ以外の R2+ は提案（プレビュー）のみ
- `report`: JSON/Markdownレポートを出力（`report --json` / `report --markdown`）
- `config`: 有効な設定を表示（`config --show`）
- `completion`: bash/zsh/fish の補完スクリプトを生成

## インストール / ビルド

このリポジトリはRustで実装されています。

```sh
cargo build
```

## 使い方

### ui（対話UI）

```sh
macdiet ui
```

### doctor（短時間診断）

```sh
macdiet doctor
macdiet doctor --top 10
macdiet doctor --json
```

`doctor` の末尾に `Snapshots:` セクション（Time Machine ローカル / APFS）も表示します。詳細な確認は `macdiet snapshots status` を使用してください。

補足: `doctor` は時間内に収めるため、サイズ推定をベストエフォート（場合により未観測/低信頼）で行います。より厳密な集計は `scan --deep` を使用してください。

### scan（詳細スキャン）

```sh
macdiet scan --deep --scope dev --max-depth 2 --top-dirs 20
macdiet scan --deep --exclude '**/node_modules/**'
macdiet scan --deep --json
```

scope presets:

- `dev`: `~/Library/Developer`, `~/Library/Caches/Homebrew`, `~/.cargo`, `~/.gradle`, `~/.npm`, `~/.pnpm-store`, `~/Library/pnpm/store`
- `userlib`: `~/Library`
- `all-readable`: `~`

### snapshots（スナップショット診断）

```sh
macdiet snapshots status
macdiet snapshots status --json
```

補足: `macdiet doctor` でも Snapshots サマリを表示します（ここだけ見たい場合は `snapshots status` 推奨）。

thin（R3、実行は慎重に。必要なら `sudo` で実行）:

```sh
macdiet --dry-run snapshots thin --bytes 50000000000 --urgency 2
macdiet snapshots thin --bytes 50000000000 --urgency 2
```

delete（R3、`diskutil apfs listSnapshots /` で検出したUUIDのみ）:

```sh
macdiet --dry-run snapshots delete --id <uuid|name>
macdiet snapshots delete --id <uuid|name>
```

`snapshots thin/delete` は実行ログを `~/.config/macdiet/logs/` に保存します（コマンド/exit/出力。R3操作の監査用）。

### fix（安全な範囲での掃除）

dry-run（既定）:

```sh
macdiet fix
macdiet fix --risk R1
macdiet fix --interactive
```

対象限定（Finding ID / Action ID で絞り込み）:

```sh
macdiet fix --target npm-cache
macdiet fix --target npm-cache-trash
macdiet fix --risk R2 --target coresimulator-devices
```

注意: `fix --apply` で実行できるのは R1/TRASH_MOVE と、allowlisted RUN_CMD のみです。その他の R2+ は提案（プレビュー）のみです（TUIの `macdiet ui` でも allowlisted RUN_CMD を限定的に実行できます）。
補足: 許可リスト外の RUN_CMD が候補に含まれている場合も、CLIは実行せず「対象外（プレビューのみ）」として扱います。

適用（R1/TRASH_MOVE + allowlisted RUN_CMD、TTY+明示確認）:

```sh
macdiet fix --apply
macdiet fix --interactive --apply
macdiet fix --risk R1 --target homebrew-cache-cleanup --apply
macdiet fix --risk R2 --target coresimulator-simctl-delete-unavailable --apply
```

`fix --apply` は実行ログを `~/.config/macdiet/logs/` に保存します（TRASH_MOVE: トランザクションログ / RUN_CMD: stdout/stderr/exit のログ）。

### config（設定）

```sh
macdiet config --show
macdiet config --show --json
```

設定ファイル:

- 既定: `~/.config/macdiet/config.toml`
- 優先順位: CLI > env (`MACDIET_*`) > config > default
- `ui.max_table_rows` は人間向け出力の表示件数（Top Findings / Actions）に反映されます

環境変数（env）:

- `MACDIET_CONFIG`: 設定ファイルパスの上書き
- `MACDIET_UI_COLOR`
- `MACDIET_UI_MAX_TABLE_ROWS`
- `MACDIET_SCAN_DEFAULT_SCOPE`
- `MACDIET_SCAN_EXCLUDE`（カンマ区切り）
- `MACDIET_FIX_DEFAULT_RISK_MAX`（`R0`..`R3`）
- `MACDIET_PRIVACY_MASK_HOME`
- `MACDIET_REPORT_INCLUDE_EVIDENCE`

### completion（補完）

```sh
macdiet completion zsh > /usr/local/share/zsh/site-functions/_macdiet
```

## JSONレポート

- `--json` はグローバルオプションです（例: `macdiet report --json`）。
- `report` の `evidence` は既定で非表示です。必要な場合は `--include-evidence` を付けてください。
- パスは既定で `~/...` にマスクします（個人情報配慮）。

## 終了コード

- 0: 成功
- 2: 引数/使い方不正（TTY必須操作を非TTYで実行、など）
- 10: 致命的エラー
- 20: 外部コマンド失敗（例: `tmutil` の失敗）

## 安全モデル（Risk Levels）

- R0: 診断のみ（既定）
- R1: ユーザー領域の安全寄りな掃除（原則 `~/.Trash` へ移動、または許可リストの RUN_CMD）
- R2: 影響が出得る（Docker/Simulator整理など）。明示確認が必須（v0.1では allowlisted RUN_CMD のみ一部実行）
- R3: スナップショットのthin/delete等。誤操作の影響が大きく、慎重な運用が必要（thin/deleteのみ一部対応）

詳細は `仕様書.md` と `SECURITY.md` を参照してください。

## ドキュメント

- `docs/system-data.md`: System Dataとは何か（一般カテゴリ）/ macdietの前提
- `docs/snapshots.md`: ローカル/ APFS スナップショットの扱いと導線
- `docs/report.md`: `report --markdown` の使い方と出力例
- `docs/ui.md`: TUI（Claude Code風）UX仕様（案、Phase 6実装済）
