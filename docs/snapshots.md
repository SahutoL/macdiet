# スナップショット（Time Machine / APFS）

macdiet はスナップショット領域を「System Dataの原因になり得る」ものとして扱いますが、誤操作の影響が大きいため **まず診断と導線の提示** を優先します。

## Time Machine ローカルスナップショット

- バックアップディスクが接続されていない状況でも復元できるよう、ローカルにスナップショットを保持することがあります。
- Apple は容量が必要な場合などに自動削除される旨を説明していますが、逼迫状況では手動対応が必要になる場合があります。

macdiet:

- `macdiet snapshots status` で存在有無を表示します（`tmutil listlocalsnapshots /` を利用）。
- 失敗時は「未観測」として表示し、確認手順（Full Disk Access / `sudo tmutil ...` など）を案内します。
- `macdiet snapshots thin --bytes <N> --urgency <1..4>` で thin を実行できます（R3、TTY+二段階確認）。
- `--dry-run` を付けると実行せずにコマンドを表示します。
- 実行には `sudo` が必要になる場合があります（ツールは自動でsudoしません）。

## APFS スナップショット

APFSスナップショットは Disk Utility で閲覧・削除できる場合があります。

macdiet:

- `diskutil apfs listSnapshots /` を試行し、失敗時は「未観測」として可視化します。
- 未観測の場合は、Disk Utility の導線に加えて `sudo diskutil ...` / Full Disk Access の確認手順も提示します。
- `macdiet snapshots delete --id <uuid|name>` で削除できます（R3、TTY+二段階確認、ツールが `diskutil` で検出し、UUIDへ一意に解決できるIDのみ受理）。
- `--dry-run` を付けると実行せずにコマンドを表示します。
- 実行には `sudo` が必要になる場合があります（ツールは自動でsudoしません）。
- `diskutil` が利用できない環境では CLI delete は実行できないため、Disk Utility の導線を優先してください。

## 「未観測」になったときの確認手順（共通）

以下は例です（環境により異なります）:

- macdiet を実行しているターミナルに Full Disk Access を許可して再実行
  - システム設定 → プライバシーとセキュリティ → フルディスクアクセス
- 可能ならコマンドを手動実行して確認
  - `tmutil listlocalsnapshots /`
  - `diskutil apfs listSnapshots /`
- 必要なら `sudo ...` を試す（ツールは自動でsudoしません）

## 重要な注意

- スナップショットの削除/薄め（thin）は影響が大きい可能性があります。
- 実行する場合は、十分に理解した上で手動手順（Disk Utility 等）を優先してください。

## JSON出力（`--json`）の見方

`macdiet snapshots status --json` は `findings[]` と `actions[]` を返します。

- `findings[].recommended_actions[].id` は、推奨アクションのID（`actions[].id` への参照）です
- `actions[].related_findings[]` は、そのアクションが関連する Finding のID（`findings[].id`）です

この2つのフィールドを使うことで「どの所見（Finding）に対する次アクションか」を機械的に追跡できます。
